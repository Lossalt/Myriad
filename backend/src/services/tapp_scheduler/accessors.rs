// Process-wide scheduler engine handle.

use super::types_frontend::TappSchedulerEngine;
use std::sync::Arc;

static SCHEDULER_ENGINE: once_cell::sync::OnceCell<Arc<TappSchedulerEngine>> =
    once_cell::sync::OnceCell::new();

/// Process-wide scheduler engine handle.
pub fn try_scheduler_engine() -> Option<Arc<TappSchedulerEngine>> {
    SCHEDULER_ENGINE.get().cloned()
}

/// Process-wide scheduler engine handle, or error string if not started.
pub fn scheduler_engine() -> Result<Arc<TappSchedulerEngine>, String> {
    try_scheduler_engine().ok_or_else(|| "Scheduler not initialized".to_string())
}

/// Initialize the process-wide scheduler engine. The engine is published
/// before it starts, so a second call finds it and starts nothing. Its loop is
/// stopped by the process job runner's shutdown.
pub fn init_scheduler(db: sea_orm::DatabaseConnection) {
    if SCHEDULER_ENGINE
        .set(Arc::new(TappSchedulerEngine::new(db)))
        .is_err()
    {
        tracing::warn!("[TappScheduler] Scheduler already initialized");
        return;
    }
    if let Some(engine) = SCHEDULER_ENGINE.get() {
        engine.start();
    }
    tracing::info!("[TappScheduler] Scheduler initialized");
}
