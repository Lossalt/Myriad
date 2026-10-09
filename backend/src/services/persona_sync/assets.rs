//! The files her entities refer to: rig packages by their content id and
//! pictures (portraits, stickers, the sticker avatar) by the sha256 of their
//! bytes. Only a file the site's persona refers to right now can be fetched,
//! so an id cannot be used to look around the site's media.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use myriad_persona_sync::{
    AssetRef, AssetType, EntityVersion, Kind, PersonaBody,
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
const PICTURE_TTL: Duration = Duration::from_secs(600);
const PICTURES_KEPT: usize = 4096;

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

async fn picture(url: &str) -> Option<(Vec<u8>, String)> {
    crate::services::image_generation::read_public_local_media(url).await
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
                    out.extend(outfit.portrait);
                    out.extend(outfit.rig);
                }
            }
            Kind::Sticker => {
                if let Ok(sticker) = entity.body_as::<StickerBody>() {
                    out.push(sticker.image);
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
