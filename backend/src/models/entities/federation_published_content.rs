//! 已发布内容映射实体
//!
//! 本地内容 → ActivityPub Activity 的映射关系

#![allow(dead_code)]

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "federation_published_content")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    /// report, phantasi-article, library, activity, tapp, dashboard
    pub content_type: String,
    #[sea_orm(column_type = "Text")]
    pub content_id: String,
    #[sea_orm(column_type = "Text")]
    pub activity_id: String,
    /// public, followers, mentioned, direct
    pub visibility: String,
    pub published_at: DateTimeWithTimeZone,
    pub updated_at: Option<DateTimeWithTimeZone>,
    /// Client `Idempotency-Key`, unique per user; NULL for unkeyed publishes.
    #[sea_orm(column_type = "Text", nullable)]
    pub idempotency_key: Option<String>,
    /// Hash of the first keyed request; a different payload under the key is 409.
    #[sea_orm(column_type = "Text", nullable)]
    pub idempotency_fingerprint: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
