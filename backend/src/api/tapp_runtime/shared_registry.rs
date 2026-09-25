//! Compatibility re-export of the services-layer Tapp registry adapter.
//!
//! Implementation: workspace crate [`myriad_tapp_registry`]. The process
//! database connection is [`crate::services::process_db`], not part of it.
//! Prefer importing `crate::services::tapp_registry` from services code.

pub use crate::services::tapp_registry::*;
