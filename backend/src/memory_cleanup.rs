//! Process-owned reclamation, including workers that never receive HTTP requests.
use std::time::Duration;

/// Both jobs live on the process job runner, so shutdown drains them with the
/// other background jobs; dropping the guard also stops them.
pub(crate) struct MemoryCleanup(
    crate::services::jobs::JobHandle,
    Option<crate::services::jobs::JobHandle>,
);

impl Drop for MemoryCleanup {
    fn drop(&mut self) {
        self.0.cancel();
        if let Some(upgrade) = &self.1 {
            upgrade.cancel();
        }
    }
}

pub(crate) fn start(automatic_media_upgrade: bool) -> MemoryCleanup {
    let cleanup = crate::services::jobs::jobs().periodic(
        "memory cleanup",
        crate::services::jobs::Every::new(Duration::from_secs(60)),
        reclaim,
    );
    MemoryCleanup(
        cleanup,
        automatic_media_upgrade.then(crate::services::media::start_upgrade_worker),
    )
}

async fn reclaim() {
    use crate::services::{agent, ai_task_runtime, analyzer};
    crate::api::github_stars::cleanup_cache();
    crate::api::game_presence::cleanup_cache();
    analyzer::cleanup_shape_memo();
    crate::services::tapp_api_service::cleanup_response_cache().await;
    agent::consciousness::cleanup_attention();
    crate::services::oauth::state::cleanup_used_nonces().await;
    crate::api::merope_rig::cleanup_verified_packages().await;
    if tokio::time::timeout(
        Duration::from_secs(30),
        agent::executor::task_store::cleanup_retained_state(),
    )
    .await
    .is_err()
    {
        tracing::warn!("Agent memory cleanup persistence timed out");
    }
    crate::services::discord_bot::cleanup_channel_types().await;
    ai_task_runtime::cleanup_local_tasks().await;
    if let Ok(db) = crate::services::tapp_registry::database() {
        match tokio::time::timeout(
            Duration::from_secs(30),
            crate::services::media::maintain(&db),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "media maintenance failed"),
            Err(_) => tracing::warn!("media maintenance timed out"),
        }
    }
}
