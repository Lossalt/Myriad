//! Her stickers: pictures of her she made herself, and stickers people sent
//! in groups she kept to send herself (see `merope::stickers`).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "merope_stickers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// What it shows, as she described it.
    pub picture: String,
    /// What it means: when she would send it.
    pub meaning: String,
    /// The group it belongs to, or none: anywhere.
    pub venue: Option<String>,
    pub asset_id: String,
    /// Who it was drawn as (`myriad_merope::stickers::identity_key`).
    pub identity: String,
    pub sent: i32,
    pub last_sent_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
    /// The group (as sessions keep it) she kept it from, when someone there
    /// sent it; none for one she made.
    pub kept_from: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
