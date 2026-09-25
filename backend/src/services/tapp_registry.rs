//! Tapp runtime registry adapter (re-export of the workspace crate).
//!
//! Pure registry/mailbox operations live in [`myriad_tapp_registry`]. This module
//! is the **services-layer** entry point for Tapp runtime state. The process
//! database connection is [`crate::services::process_db`], not a Tapp concern.
//!
//! `api::tapp_runtime::shared_registry` is a thin re-export of this module for
//! path stability in HTTP handlers only — services must import this path.

pub use myriad_tapp_registry::*;
