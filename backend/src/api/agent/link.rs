//! `GET /api/agent/link`: her link, for a device. `sync_device_link_auth`
//! has checked the device token before the upgrade; a refusal is a plain
//! HTTP answer and no upgrade. The link itself is
//! [`crate::services::agent::link`].

use axum::{
    Extension,
    extract::{State, ws::WebSocketUpgrade},
    response::Response,
};
use sea_orm::DatabaseConnection;

use crate::middleware::sync_device::SyncDevice;
use crate::services::agent::link;

pub async fn her_link(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let limit = link::max_frame_bytes();
    upgrade
        .max_message_size(limit)
        .max_frame_size(limit)
        .on_upgrade(move |socket| link::serve(socket, db, device.0))
}
