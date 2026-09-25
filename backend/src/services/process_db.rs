//! The process's database connection, for code that runs outside a request.
//!
//! One slot, shared with [`crate::state::AppState`]: connect, reload and
//! health reconnect write it once, and background services and HTTP
//! extractors read the same handle — no dual live pools. Platform
//! infrastructure: every subsystem (Tapp runtime, agent, channels, workers)
//! depends on it; it depends on none of them.

use sea_orm::{DatabaseConnection, DbErr};
use std::sync::{Arc, OnceLock, RwLock};

/// Single shared DB slot for full-mode process + [`crate::state::AppState`].
///
/// Reconnect / reload writes here once; both `database()` (background) and
/// `AppState` / `extract::Db` (HTTP) read the same handle — no dual-pool fork.
static PROCESS_DB: OnceLock<Arc<RwLock<Option<DatabaseConnection>>>> = OnceLock::new();

/// Shared slot used by process helpers and `AppState::from_shared`.
pub fn shared_database_slot() -> Arc<RwLock<Option<DatabaseConnection>>> {
    PROCESS_DB
        .get_or_init(|| Arc::new(RwLock::new(None)))
        .clone()
}

/// Wire (or re-wire) the process + AppState DB after connect / reload / health reconnect.
pub fn set_process_database(db: DatabaseConnection) {
    let slot = shared_database_slot();
    *slot
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(db);
}

/// Process-global DB connection for services that run outside a request.
///
/// HTTP handlers must use `State` / `extract::Db` (same underlying slot after
/// `AppState::from_shared`). Prefer passing an explicit `DatabaseConnection` on
/// request paths (grant / rate limit / ws ticket).
pub fn database() -> Result<DatabaseConnection, DbErr> {
    shared_database_slot()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| DbErr::Custom("database is not connected".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_slot_is_same_arc_for_process_and_appstate_style_holders() {
        let a = shared_database_slot();
        let b = shared_database_slot();
        assert!(Arc::ptr_eq(&a, &b), "slot must be a single process Arc");
    }
}
