//! The platform runtime registry, as the Tapp runtime's HTTP handlers use it.
//!
//! Implementation: workspace crate [`myriad_runtime_registry`]. The process
//! database connection is [`crate::services::process_db`], not part of it.
//! Prefer importing `crate::services::runtime_registry` from services code.

pub use crate::services::runtime_registry::*;
