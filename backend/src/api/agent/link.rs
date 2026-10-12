//! `GET /api/agent/link`: her link, for a device. `sync_device_link_auth`
//! has checked the device token before the upgrade; a refusal is a plain
//! HTTP answer and no upgrade. While she is moving or lives on another end
//! (her host lease), the upgrade is refused too, `host_frozen` or
//! `host_moved`; a plain GET without the upgrade gets the same answer, which
//! is how a child reads why. The link itself is
//! [`crate::services::agent::link`].

use axum::{
    Extension, Json,
    extract::{
        State,
        ws::{WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::middleware::sync_device::SyncDevice;
use crate::services::agent::link;
use crate::services::persona_sync::lease;

pub async fn her_link(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    match lease::link_refusal(&db).await {
        Ok(None) => {}
        Ok(Some((code, body))) => {
            tracing::info!(device = %device.0.id, code, "[link] refused: she is not here");
            return (StatusCode::FORBIDDEN, Json(body)).into_response();
        }
        Err(error) => {
            tracing::error!(%error, "[link] could not read her host lease");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Could not open her link", "code": "internal_error" })),
            )
                .into_response();
        }
    }
    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => return rejection.into_response(),
    };
    let limit = link::max_frame_bytes();
    upgrade
        .max_message_size(limit)
        .max_frame_size(limit)
        .on_upgrade(move |socket| link::serve(socket, db, device.0))
}
