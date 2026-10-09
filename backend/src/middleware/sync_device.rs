//! `sync_device_auth`: the gate in front of the routes a device uses to sync
//! her (`/api/agent/sync/*`).
//!
//! It accepts only a device token (`Authorization: Bearer mys_…`) and never
//! a session: those routes are for devices. The other way round holds by
//! construction, since every session route verifies a JWT signature and a
//! device token has none. A device of a previous site owner is revoked the
//! moment it is used. Neither the token nor its hash appears in any answer
//! or log.

use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::services::persona_sync::devices::{self, Device, Refused};

/// The device a sync request comes from.
#[derive(Debug, Clone)]
pub struct SyncDevice(pub Device);

fn refuse(status: StatusCode, error: &str, code: &str) -> Response {
    (status, Json(json!({ "error": error, "code": code }))).into_response()
}

pub async fn sync_device_auth(
    State(db): State<DatabaseConnection>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(token) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_string)
    else {
        return refuse(
            StatusCode::UNAUTHORIZED,
            "A device token is required",
            "device_token_required",
        );
    };
    let owner = match crate::services::principal::site_owner_id(&db).await {
        Ok(owner) => owner,
        Err(error) => {
            tracing::error!(%error, "[sync] could not resolve the site owner");
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not check the device",
                "internal_error",
            );
        }
    };
    let device = match devices::authenticate(&db, &token, owner).await {
        Ok(device) => device,
        Err(Refused::Unknown) => {
            return refuse(
                StatusCode::UNAUTHORIZED,
                "Unknown device token",
                "device_token_invalid",
            );
        }
        Err(Refused::Revoked) => {
            return refuse(
                StatusCode::UNAUTHORIZED,
                "This device was removed from the site",
                "device_revoked",
            );
        }
        Err(Refused::OwnerChanged) => {
            return refuse(
                StatusCode::FORBIDDEN,
                "The site has a new owner; this device was removed",
                "owner_changed",
            );
        }
        Err(Refused::NoOwner) => {
            return refuse(
                StatusCode::FORBIDDEN,
                "The site has no owner yet",
                "site_owner_required",
            );
        }
        Err(Refused::Db(error)) => {
            tracing::error!(%error, "[sync] could not check a device token");
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not check the device",
                "internal_error",
            );
        }
    };
    if let Err(error) = devices::seen(&db, &device.id).await {
        tracing::warn!(%error, device = %device.id, "[sync] could not record a device as seen");
    }
    req.extensions_mut().insert(SyncDevice(device));
    next.run(req).await
}
