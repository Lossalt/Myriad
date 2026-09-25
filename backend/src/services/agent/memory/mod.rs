//! Agent long-term memory: one table (`agent_memories`) for Chat and Work.
//!
//! [`unified`] owns storage, audience and retrieval; `lexical` scores text
//! relevance for it; [`work_memory`] turns a finished Work run into memories
//! and imports the pre-unified JSON store.

mod lexical;
pub mod unified;
pub(crate) mod work_memory;

pub(crate) use work_memory::import_legacy_json;
