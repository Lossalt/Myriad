//! The files her entities refer to: rig packages by their content id and
//! pictures (portraits, stickers, the sticker avatar) by the sha256 of their
//! bytes. Only a file the site's persona refers to right now can be fetched,
//! so an id cannot be used to look around the site's media.
//!
//! A picture at an outside address is fetched into the site's image cache
//! the first time a device reads her, through the outbound guards (public
//! addresses only, DNS pinned, no redirects, at most 10 MiB, image bytes
//! only), and read from there after: the device never fetches an outside
//! address, and the picture stays readable when the address stops
//! answering. Lazily, when a snapshot or an asset is asked for, because only
//! then does anyone need the copy: a site with no devices fetches nothing,
//! her persona is not rewritten (a pull writes none of her data), and there
//! is no job to keep in step with every place a picture is set. One that
//! cannot be fetched is tried again after [`PICTURE_TTL`]; until then, and
//! when it truly cannot be read, the entity carries the mark
//! `{"unreadable": true}` in its place.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use myriad_persona_sync::{
    AssetRef, AssetType, EntityVersion, Kind, PersonaBody, Picture,
    canonical::sha256_hex,
    entity::{OutfitBody, StickerBody},
};
use sea_orm::{ConnectionTrait, DbErr, EntityTrait};

use crate::services::merope_rig;

/// What a rig asset is served as: its manifest and atlas, in that order.
pub const RIG_MEDIA_TYPE: &str = "multipart/mixed";

/// Pictures read lately, by address: hashing every sticker for every page
/// would read them all each time.
static PICTURES: LazyLock<Mutex<HashMap<String, (Instant, AssetRef)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub const PICTURE_TTL: Duration = Duration::from_secs(600);
const PICTURES_KEPT: usize = 4096;

/// Outside pictures that could not be fetched lately, by the sha256 of
/// their address: a snapshot of many pages does not wait on a dead host
/// once per page.
static UNFETCHABLE: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// One fetch at a time: the image cache writes a file in place, and a
/// second reader must not hash it half written.
static FETCHING: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// The picture at a site media address, as an asset. None when it is not a
/// local file the site can read.
pub async fn image_ref(url: &str) -> Option<AssetRef> {
    if let Ok(kept) = PICTURES.lock()
        && let Some((at, asset)) = kept.get(url)
        && at.elapsed() < PICTURE_TTL
    {
        return Some(asset.clone());
    }
    let (bytes, media_type) = picture(url).await?;
    let asset = AssetRef {
        id: sha256_hex(&bytes),
        kind: AssetType::Image,
        bytes: bytes.len() as u64,
        media_type,
    };
    if let Ok(mut kept) = PICTURES.lock() {
        if kept.len() >= PICTURES_KEPT {
            kept.clear();
        }
        kept.insert(url.to_string(), (Instant::now(), asset.clone()));
    }
    Some(asset)
}

/// A picture field as it crosses: the image, or the mark that the site has
/// a picture there and cannot read it.
pub async fn picture_at(url: &str) -> Picture {
    match image_ref(url).await {
        Some(asset) => Picture::Image(asset),
        None => Picture::unreadable(),
    }
}

async fn picture(url: &str) -> Option<(Vec<u8>, String)> {
    if let Some(found) = crate::services::image_generation::read_public_local_media(url).await {
        return Some(found);
    }
    fetched(&crate::services::image_cache::ImageCacheService::new(), url).await
}

/// An outside picture (`http`/`https`), from the image cache, fetched into
/// it first when it is not there yet. None when it cannot be fetched or is
/// not an image. Neither the address nor anything in it is logged: an
/// address can carry a signature.
pub(crate) async fn fetched(
    cache: &crate::services::image_cache::ImageCacheService,
    url: &str,
) -> Option<(Vec<u8>, String)> {
    let url = url.trim();
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str().unwrap_or("-").to_string();
    let key = sha256_hex(url.as_bytes());
    if let Ok(failed) = UNFETCHABLE.lock()
        && failed
            .get(&key)
            .is_some_and(|at| at.elapsed() < PICTURE_TTL)
    {
        return None;
    }
    let read = {
        let _one = FETCHING.lock().await;
        let local = match cache.get_cached_url(url).await {
            Some(local) => Ok(local),
            None => cache.cache_image(url).await,
        };
        match local {
            Ok(local) => match cache.read_local_public_url(&local).await {
                Ok((bytes, _)) => match crate::services::agent::chat_attachments::sniff(&bytes) {
                    Some(media_type) => Ok((bytes, media_type.to_string())),
                    None => Err("not an image".to_string()),
                },
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        }
    };
    match read {
        Ok(found) => {
            if let Ok(mut failed) = UNFETCHABLE.lock() {
                failed.remove(&key);
            }
            Some(found)
        }
        Err(error) => {
            tracing::warn!(%host, %error, "[sync] an outside picture cannot be read");
            if let Ok(mut failed) = UNFETCHABLE.lock() {
                if failed.len() >= PICTURES_KEPT {
                    failed.clear();
                }
                failed.insert(key, Instant::now());
            }
            None
        }
    }
}

/// A compiled rig the site keeps, as an asset.
pub async fn rig_ref(id: &str) -> Option<AssetRef> {
    let id = merope_rig::normalize_asset_id(id)?;
    let manifest = tokio::fs::metadata(merope_rig::manifest_path(&id))
        .await
        .ok()?;
    let atlas = tokio::fs::metadata(merope_rig::atlas_path(&id))
        .await
        .ok()?;
    Some(AssetRef {
        id,
        kind: AssetType::Rig,
        bytes: manifest.len() + atlas.len(),
        media_type: RIG_MEDIA_TYPE.to_string(),
    })
}

/// Every asset `entities` refer to, once, by id.
pub fn referenced(entities: &[EntityVersion]) -> Vec<AssetRef> {
    let mut out: Vec<AssetRef> = Vec::new();
    for entity in entities {
        match entity.kind {
            Kind::Outfit => {
                if let Ok(outfit) = entity.body_as::<OutfitBody>() {
                    out.extend(outfit.portrait.as_ref().and_then(Picture::asset).cloned());
                    out.extend(outfit.rig);
                }
            }
            Kind::Sticker => {
                if let Ok(sticker) = entity.body_as::<StickerBody>() {
                    out.extend(sticker.image.asset().cloned());
                }
            }
            Kind::Persona => {
                if let Ok(persona) = entity.body_as::<PersonaBody>()
                    && let Some(field) = persona.fields.get("stickerAvatar")
                    && let Ok(asset) = serde_json::from_value::<AssetRef>(field.value.clone())
                {
                    out.push(asset);
                }
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// Where an asset the persona refers to is kept.
pub enum Source {
    Rig { id: String },
    Image { url: String, media_type: String },
}

/// The asset `id`, if the site's persona refers to it now: one of her
/// outfits' portrait or rig, the sticker avatar, or one of her stickers
/// (not a group's).
pub async fn find<C: ConnectionTrait>(db: &C, id: &str) -> Result<Option<Source>, DbErr> {
    let Some(id) = merope_rig::normalize_asset_id(id) else {
        return Ok(None);
    };
    let Some(row) = crate::models::entities::agent_persona::Entity::find_by_id(
        crate::services::agent::merope::api::store::PERSONA_ROW_ID,
    )
    .one(db)
    .await?
    else {
        return Ok(None);
    };
    let mut pictures: Vec<String> = Vec::new();
    let wardrobe = row
        .visual_profile
        .as_ref()
        .and_then(|profile| profile.get("wardrobe"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in &wardrobe {
        if let Some(rig) = item.get("rigAssetId").and_then(serde_json::Value::as_str)
            && merope_rig::normalize_asset_id(rig).as_deref() == Some(id.as_str())
            && rig_ref(&id).await.is_some()
        {
            return Ok(Some(Source::Rig { id }));
        }
        if let Some(url) = item
            .get("portraitAssetId")
            .and_then(serde_json::Value::as_str)
        {
            pictures.push(url.to_string());
        }
    }
    pictures.extend(row.portrait_asset_id.clone());
    pictures.extend(row.avatar_asset_id.clone());
    let stickers = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT asset_id FROM merope_stickers WHERE venue IS NULL".to_string(),
        ))
        .await?;
    for sticker in &stickers {
        pictures.push(sticker.try_get("", "asset_id")?);
    }
    for url in pictures {
        let url = url.trim();
        if url.is_empty() {
            continue;
        }
        if let Some(asset) = image_ref(url).await
            && asset.id == id
        {
            return Ok(Some(Source::Image {
                url: url.to_string(),
                media_type: asset.media_type,
            }));
        }
    }
    Ok(None)
}

/// The bytes of a picture the persona refers to, checked against its id
/// again: the file may have changed since it was hashed.
pub async fn image_bytes(url: &str, id: &str) -> Option<Vec<u8>> {
    let (bytes, _) = picture(url).await?;
    (sha256_hex(&bytes) == id).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::services::image_cache::ImageCacheService;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-not-much-of-a-picture";

    /// A local stand-in for outside hosts, counting requests by path.
    async fn fake_host() -> (String, Arc<Mutex<HashMap<String, usize>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
        let counted = hits.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let counted = counted.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..n]);
                    }
                    let line = String::from_utf8_lossy(&request);
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    *counted.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let (status, media_type, body): (&str, &str, Vec<u8>) = match path.as_str() {
                        "/ok.png" | "/guarded.png" => ("200 OK", "image/png", PNG.to_vec()),
                        "/page" => ("200 OK", "text/html", b"<html></html>".to_vec()),
                        "/fake.png" => ("200 OK", "image/png", b"<html></html>".to_vec()),
                        _ => ("404 Not Found", "text/plain", b"gone".to_vec()),
                    };
                    let length = if path == "/big.png" {
                        // Declared past the limit: refused before the body.
                        11 * 1024 * 1024
                    } else {
                        body.len()
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {media_type}\r\n\
                         Content-Length: {length}\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (base, hits)
    }

    fn hits(of: &Arc<Mutex<HashMap<String, usize>>>, path: &str) -> usize {
        of.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    /// Outside pictures through the outbound guards, against a local fake
    /// host only: fetched once into the cache and read from there after; a
    /// dead address, a page, bytes that are no image and an oversized body
    /// are unreadable, and a dead address is not asked again at once.
    #[tokio::test]
    async fn an_outside_picture_is_fetched_once_into_the_cache() {
        let _lab = crate::services::outbound_security::tests_lab_env_lock().await;
        let previous_lab = std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND").ok();
        let previous_env = std::env::var("ENVIRONMENT").ok();
        unsafe { std::env::remove_var("ENVIRONMENT") };
        unsafe { std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND") };
        let dir = std::env::temp_dir().join(format!("sync-pictures-{}", uuid::Uuid::new_v4()));
        let cache = ImageCacheService::at(dir.clone());
        let (base, served) = fake_host().await;

        // The guards hold: a loopback address is no outside picture.
        assert!(
            fetched(&cache, &format!("{base}/guarded.png"))
                .await
                .is_none()
        );
        assert_eq!(hits(&served, "/guarded.png"), 0);

        // The fake host stands in for the outside from here on.
        unsafe { std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1") };
        let ok = format!("{base}/ok.png");
        let (bytes, media_type) = fetched(&cache, &ok).await.unwrap();
        assert_eq!((bytes.as_slice(), media_type.as_str()), (PNG, "image/png"));
        let (again, _) = fetched(&cache, &format!("  {ok} ")).await.unwrap();
        assert_eq!(again, bytes);
        assert_eq!(hits(&served, "/ok.png"), 1, "read from the cache after");

        for path in ["/gone.png", "/page", "/fake.png", "/big.png"] {
            let url = format!("{base}{path}");
            assert!(fetched(&cache, &url).await.is_none(), "{path}");
            assert!(fetched(&cache, &url).await.is_none(), "{path}");
            assert_eq!(hits(&served, path), 1, "{path} is not asked again at once");
        }
        assert!(fetched(&cache, "ftp://127.0.0.1/x.png").await.is_none());
        assert!(fetched(&cache, "/media/assets/nope/x.png").await.is_none());

        match previous_lab {
            Some(value) => unsafe {
                std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", value)
            },
            None => unsafe { std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND") },
        }
        if let Some(value) = previous_env {
            unsafe { std::env::set_var("ENVIRONMENT", value) };
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
