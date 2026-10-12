//! Stickers people sent in groups that she kept to send herself: one she
//! would want, as she is, when she first looked at it (see
//! `myriad_merope::seeing::sticker_prompt`). One only that group would get
//! is sent only there; the rest anywhere. Kept as a picture of their own,
//! since the app's link to it does not last; let go when she has not sent
//! one for a while.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, Set,
};

use crate::models::entities::merope_stickers::{self, Column, Entity, Model};
use crate::services::image_generation::{self, GeneratedImage, GeneratedImageSource};

/// What a kept sticker is drawn as: not her, so whatever she looks like.
pub(super) const KEPT: &str = "kept";
/// Kept stickers still in use, at most.
const AT_MOST: u64 = 100;
/// One kept and not sent for this long is let go.
const FADES_AFTER_DAYS: i64 = 60;
/// Kept stickers are no larger than this across.
const EDGE: u32 = 512;

/// The kept stickers still in use: kept or sent lately.
pub(super) fn in_use() -> Condition {
    let since = Utc::now().fixed_offset() - chrono::Duration::days(FADES_AFTER_DAYS);
    Condition::all().add(Column::KeptFrom.is_not_null()).add(
        Condition::any()
            .add(Column::CreatedAt.gte(since))
            .add(Column::LastSentAt.gte(since)),
    )
}

/// Keep the sticker someone sent in the group `venue` (as sessions keep it),
/// when she would: what she saw of it, and its bytes. Billed to `owner`'s
/// media.
pub async fn keep(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    seen: &myriad_merope::seeing::Seen,
    bytes: Vec<u8>,
) -> Option<Model> {
    if !seen.keep {
        return None;
    }
    let meaning = seen.says.clone().unwrap_or_else(|| seen.what.clone());
    let already = Entity::find()
        .filter(Column::KeptFrom.is_not_null())
        .filter(Column::Picture.eq(seen.what.as_str()))
        .filter(Column::Meaning.eq(meaning.as_str()))
        .count(db)
        .await
        .unwrap_or(0);
    if already > 0 {
        return None;
    }
    if Entity::find()
        .filter(in_use())
        .count(db)
        .await
        .unwrap_or(AT_MOST)
        >= AT_MOST
    {
        tracing::info!("[Merope] has as many stickers kept as she uses");
        return None;
    }
    let png = tokio::task::spawn_blocking(move || as_png(&bytes))
        .await
        .ok()
        .flatten()?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let actor = crate::services::media::MediaActor::admin(owner).ok()?;
    let persisted = image_generation::persist_generated_with_status(
        db,
        crate::services::media::MediaContext::site(
            actor,
            crate::services::media::MediaSource::Channel,
        )
        .with_producer_key(format!("merope-kept-sticker:{id}")),
        GeneratedImage {
            source: GeneratedImageSource::Inline(png),
            media_type: "image/png".to_string(),
            width: 0,
            height: 0,
        },
        "sticker",
        crate::services::media::MediaExposure::Public,
    )
    .await
    .ok()?;
    let row = merope_stickers::ActiveModel {
        id: Set(id),
        picture: Set(seen.what.clone()),
        meaning: Set(meaning),
        venue: Set(seen.only_there.then(|| venue.to_string())),
        asset_id: Set(persisted.url),
        identity: Set(KEPT.to_string()),
        sent: Set(0),
        last_sent_at: Set(None),
        created_at: Set(Utc::now().fixed_offset()),
        kept_from: Set(Some(venue.to_string())),
    };
    match row.insert(db).await {
        Ok(model) => {
            tracing::info!(%venue, only_there = seen.only_there, "[Merope] kept a sticker someone sent");
            Some(model)
        }
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not keep a sticker someone sent");
            None
        }
    }
}

/// Its first frame as PNG, no larger than `EDGE` across.
fn as_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > EDGE {
        image.resize(EDGE, EDGE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .to_rgba8()
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_kept_sticker_is_its_first_frame_small() {
        let mut gif = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(800, 400))
            .write_to(&mut std::io::Cursor::new(&mut gif), image::ImageFormat::Gif)
            .unwrap();
        let png = super::as_png(&gif).unwrap();
        let image = image::load_from_memory(&png).unwrap();
        assert_eq!((image.width(), image.height()), (512, 256));
        assert_eq!(super::as_png(b"not a picture"), None);
    }
}
