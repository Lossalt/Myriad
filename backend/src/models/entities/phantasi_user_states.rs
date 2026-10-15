//! Phantasi 阅读 - 用户阅读状态实体
//!
//! 存储用户的已读、收藏等状态，支持多用户同步

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "phantasi_user_states")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 用户 ID
    pub user_id: i32,
    /// 文章 ID
    pub item_id: i32,
    /// 是否已读
    pub is_read: bool,
    /// 是否收藏
    pub is_starred: bool,
    /// 阅读时间
    pub read_at: Option<DateTimeWithTimeZone>,
    /// 收藏时间
    pub starred_at: Option<DateTimeWithTimeZone>,
    /// 用户笔记
    #[sea_orm(column_type = "Text", nullable)]
    pub notes: Option<String>,
    pub updated_at: DateTimeWithTimeZone,
    /// Server-owned version; database trigger increments on every update.
    pub revision: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::phantasi_items::Entity",
        from = "Column::ItemId",
        to = "super::phantasi_items::Column::Id"
    )]
    PhantasiItem,
}

impl Related<super::phantasi_items::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PhantasiItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// 批量标记已读请求
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarkAllReadRequest {
    /// 订阅源 ID（可选，不传则标记所有）
    pub source_id: Option<i32>,
    /// 分类（可选）
    pub category: Option<String>,
    /// 截止时间（可选，标记此时间之前的文章）
    pub before: Option<i64>,
}
