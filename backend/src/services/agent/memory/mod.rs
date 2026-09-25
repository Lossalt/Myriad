//! Agent long-term memory: one table (`agent_memories`) for Chat and Work.
//!
//! [`unified`] owns storage, audience and retrieval; [`work_memory`] turns a
//! finished Work run into memories and imports the pre-unified JSON store.

pub mod unified;
pub(crate) mod work_memory;

pub(crate) use work_memory::import_legacy_json;
