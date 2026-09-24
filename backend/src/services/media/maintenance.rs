//! Bounded maintenance, owned by the process cleanup loop on every worker.
use super::{MediaError, MediaService};
use crate::models::entities::media_assets;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

pub async fn maintain(db: &DatabaseConnection) -> Result<(), MediaError> {
    let service = MediaService::from_data_paths(crate::services::data_paths::paths());
    let recovery = service.recover_expired(db, 16).await;
    let deletions = retry_deletions(&service, db, 16).await;
    let urls = normalize_catalog_urls(db, 64).await;
    let refs = prune_references(db, 500).await;
    recovery
        .map(|_| ())
        .and(deletions)
        .and(urls.map(|_| ()))
        .and(refs.map(|_| ()))
}

/// References whose consumer can no longer show the media. Conversation
/// messages go away by cascade with their session, so their references are
/// pruned here rather than at every delete site. Expired references are kept
/// a day for diagnosis. Run inputs bound before they expired on their own are
/// dropped once the run is long over. Bounded per tick.
pub async fn prune_references(db: &DatabaseConnection, limit: u64) -> Result<u64, MediaError> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
DELETE FROM media_references WHERE id IN (
    SELECT r.id FROM media_references r
    WHERE (r.expires_at IS NOT NULL AND r.expires_at < NOW() - interval '1 day')
       OR (r.consumer_type = 'channel_message'
           AND r.consumer_id ~ '^agent_messages:[0-9]+$'
           AND NOT EXISTS (SELECT 1 FROM agent_messages m
                           WHERE m.id = CASE WHEN r.consumer_id ~ '^agent_messages:[0-9]+$'
                                        THEN substring(r.consumer_id FROM 16)::int END))
       OR (r.consumer_type = 'channel_message'
           AND r.consumer_id ~ '^federation_channel_messages:[0-9]+$'
           AND NOT EXISTS (SELECT 1 FROM federation_channel_messages m
                           WHERE m.id = CASE WHEN r.consumer_id ~ '^federation_channel_messages:[0-9]+$'
                                        THEN substring(r.consumer_id FROM 29)::int END))
       OR (r.consumer_type = 'tapp_storage'
           AND NOT EXISTS (SELECT 1 FROM tapp_storage s
                           WHERE s.id = CASE WHEN r.consumer_id ~ '^[0-9]+$'
                                        THEN r.consumer_id::int END))
       OR (r.consumer_type = 'channel_message'
           AND r.consumer_id LIKE 'run\_%'
           AND r.expires_at IS NULL
           AND r.created_at < NOW() - interval '1 day')
    LIMIT $1
)
"#,
            [(limit.clamp(1, 5000) as i64).into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

/// Private assets written before every asset had one permanent address kept
/// `/api/media/{id}/content` in the catalog row. That alias is still served;
/// the row itself should carry the permanent address. Bounded and idempotent.
pub(super) async fn normalize_catalog_urls(
    db: &DatabaseConnection,
    limit: u64,
) -> Result<u64, MediaError> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = media_assets::Entity::find()
        .filter(media_assets::Column::State.eq("ready"))
        .filter(media_assets::Column::Url.starts_with("/api/media/"))
        // Rows it cannot fix must not occupy every batch and stall the rest.
        .filter(media_assets::Column::PublicId.is_not_null())
        .order_by_asc(media_assets::Column::Id)
        .limit(limit.clamp(1, 256))
        .all(db)
        .await?;
    let mut updated = 0;
    for row in rows {
        let Some(public_id) = row.public_id else {
            continue;
        };
        let filename = super::urls::filename_for_mime(&row.name, &row.mime, public_id)?;
        let result = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE media_assets SET url = $1 WHERE id = $2 AND url = $3",
                [
                    super::urls::compatible_url(public_id, &filename).into(),
                    row.id.into(),
                    row.url.clone().into(),
                ],
            ))
            .await?;
        updated += result.rows_affected();
    }
    Ok(updated)
}

pub(super) async fn retry_deletions(
    service: &MediaService,
    db: &DatabaseConnection,
    limit: u64,
) -> Result<(), MediaError> {
    let rows = media_assets::Entity::find()
        .filter(media_assets::Column::State.eq("deleting"))
        .order_by_asc(media_assets::Column::UpdatedAt)
        .limit(limit.clamp(1, 32))
        .all(db)
        .await?;
    for row in rows {
        // Touch before retry so a permanently failing file cannot starve others.
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE media_assets SET updated_at = NOW() WHERE id = $1 AND state = 'deleting'",
            [row.id.into()],
        ))
        .await?;
        if let Err(error) = service.delete(db, row.id).await {
            tracing::warn!(asset_id = row.id, %error, "media deletion retry failed");
        }
    }
    Ok(())
}

/// Owned by the process lifecycle; never awaited by database/schema startup.
pub fn start_upgrade_worker() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async {
        loop {
            // Give initialization time to connect the DB. No elapsed-time test
            // marks the migration complete; its durable cursor is authoritative.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let Ok(db) = crate::services::tapp_registry::database() else {
                continue;
            };
            let service = MediaService::from_data_paths(crate::services::data_paths::paths());
            let legacy = super::LegacyPaths::from_data_paths(crate::services::data_paths::paths());
            let origins = super::upgrade::configured_origins().await;
            match super::upgrade::automatic_step(
                &db,
                service.store(),
                &legacy,
                &origins,
                chrono::Utc::now().timestamp(),
            )
            .await
            {
                Ok(Some(progress)) if progress.complete => {
                    tracing::info!(
                        scanned = progress.scanned,
                        unresolved = progress.unresolved,
                        "media upgrade completed"
                    )
                }
                Ok(Some(progress)) if progress.error.is_some() => tracing::warn!(
                    error = ?progress.error, source = ?progress.error_source, pending_failures = progress.pending_failures, next_retry_at = ?progress.next_retry_at,
                    "media upgrade has deferred records; normal records continue before retry"),
                Ok(_) => {}
                Err(error) => {
                    // Disconnected/uninitialized DB cannot persist its backoff yet.
                    tracing::warn!(%error, "media upgrade unavailable; retrying later");
                    tokio::time::sleep(std::time::Duration::from_secs(55)).await;
                }
            }
        }
    })
}
