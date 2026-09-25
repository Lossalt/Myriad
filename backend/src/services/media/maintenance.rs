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
///
/// 已撤回的联邦发布：撤回现在在同一事务里释放以原 Create 为消费者的引用，
/// 但更早撤回的帖子没有，升级任务回填历史活动时也会给它们重新绑上。判定只看
/// 两件确定的事：已发布行已经不在，且同一用户有一条以该 Create 为对象的本地
/// Delete（只有撤回会写）。还在已发布列表里的、从没撤回过的都不碰。
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
       OR (r.consumer_type = 'federation_activity'
           AND NOT EXISTS (SELECT 1 FROM federation_published_content p
                           WHERE p.activity_id = r.consumer_id)
           AND EXISTS (SELECT 1 FROM federation_activities c
                       WHERE c.activity_id = r.consumer_id
                         AND c.is_local AND c.activity_type = 'Create'
                         AND (c.user_id, c.activity_id) IN (
                             SELECT d.user_id,
                                    COALESCE(d.object_json #>> '{object,id}',
                                             d.object_json ->> 'object')
                             FROM federation_activities d
                             WHERE d.is_local AND d.activity_type = 'Delete')))
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

/// Seconds between upgrade steps, measured from the end of the previous step.
const UPGRADE_STEP_PAUSE: std::time::Duration = std::time::Duration::from_secs(5);
/// Steps skipped after the database was unavailable: 11 × 5s plus the regular
/// pause keeps the previous one-minute retry cadence.
const UPGRADE_UNAVAILABLE_SKIPS: u32 = 11;

/// Owned by the process lifecycle; never awaited by database/schema startup.
///
/// Periodic, not one-shot: completion is not final. A `REVISION` bump or an
/// admin restart (`upgrade::advance(.., restart = true)`) reopens the durable
/// job and relies on this loop to carry it on, so it keeps probing after
/// completion. A completed or backed-off job costs one short transaction per
/// step (`upgrade::automatic_step` returns before doing work). It runs on the
/// process job runner: shutdown lets an in-flight step finish before the drain
/// deadline, and the durable cursor makes an aborted step resumable.
pub fn start_upgrade_worker() -> crate::services::jobs::JobHandle {
    start_upgrade_job(crate::services::jobs::jobs(), UPGRADE_STEP_PAUSE)
}

pub(super) fn start_upgrade_job(
    runner: &crate::services::jobs::JobRunner,
    pause: std::time::Duration,
) -> crate::services::jobs::JobHandle {
    // Give initialization time to connect the DB. No elapsed-time test marks
    // the migration complete; its durable cursor is authoritative.
    let every = crate::services::jobs::Every::new(pause)
        .after(pause)
        .spaced();
    let skips = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    runner.periodic("media upgrade", every, move || {
        let skips = skips.clone();
        async move { upgrade_step(&skips).await }
    })
}

async fn upgrade_step(skips: &std::sync::atomic::AtomicU32) {
    use std::sync::atomic::Ordering;
    if skips
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
        .is_ok()
    {
        return;
    }
    let Ok(db) = crate::services::process_db::database() else {
        return;
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
            skips.store(UPGRADE_UNAVAILABLE_SKIPS, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn upgrade_job_is_registered_on_the_runner_and_stops_with_it() {
        let runner = crate::services::jobs::JobRunner::new();
        // A long pause keeps the step (and its process database) out of the test.
        let handle = start_upgrade_job(&runner, Duration::from_secs(3600));
        assert!(!handle.is_cancelled());
        tokio::time::timeout(
            Duration::from_secs(2),
            runner.shutdown(Duration::from_secs(1)),
        )
        .await
        .expect("upgrade job must stop with the runner");
        assert!(handle.is_cancelled());
    }

    #[tokio::test]
    async fn unavailable_backoff_skips_steps_without_touching_the_database() {
        let skips = AtomicU32::new(2);
        upgrade_step(&skips).await;
        upgrade_step(&skips).await;
        assert_eq!(skips.load(Ordering::Relaxed), 0);
    }
}
