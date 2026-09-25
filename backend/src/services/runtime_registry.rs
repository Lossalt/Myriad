//! The platform runtime registry (re-export of the workspace crate).
//!
//! Short-lived state shared by every backend replica: leases, mailboxes and
//! expiring records. Platform infrastructure with many tenants — agent runs
//! and work checkpoints, AI tasks, IM channels, rate limits, and the Tapp
//! runtime, whose rows carry their `tapp_id`. None of them owns it.
//! Operations live in [`myriad_runtime_registry`]; the process database
//! connection is [`crate::services::process_db`].
//!
//! `api::tapp_runtime::shared_registry` re-exports this module for the Tapp
//! runtime's HTTP handlers; services import this path.

pub use myriad_runtime_registry::*;
