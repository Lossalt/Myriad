//! Hidden proactive speech, one stream per addressee.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_proactive_messages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub user_id: i32,
    pub role: String,
    #[sea_orm(column_type = "Text")]
    pub content: String,
    #[sea_orm(nullable)]
    pub event_key: Option<String>,
    pub notified: bool,
    pub created_at: DateTimeWithTimeZone,
    /// The `say` it went out as on a native child, when it did.
    #[sea_orm(nullable)]
    pub say_id: Option<String>,
    /// How it was said, as on her replies: `cutOff` and `heardChars` when it
    /// was cut off aloud.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
