//! The site owner's devices that sync her: add one (its token is shown this
//! once), list them, remove one. Session routes, owner only.

use super::*;
use crate::error::HttpError;
use crate::services::persona_sync::devices::{self, AddError, Device, RevokeReason};
use axum::response::IntoResponse;
use myriad_error::AppError;

fn store_failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, "[sync] device store failed");
    HttpError(AppError::internal("Failed to update devices"))
}

/// A device as the owner's settings page shows it.
pub(crate) fn device_json(device: &Device, now: chrono::DateTime<Utc>) -> Value {
    json!({
        "id": device.id,
        "name": device.name,
        "tokenHint": device.token_hint,
        "role": device.role.as_str(),
        "createdAt": device.created_at.to_rfc3339(),
        "lastSeenAt": device.last_seen_at.map(|at| at.to_rfc3339()),
        "revokedAt": device.revoked_at.map(|at| at.to_rfc3339()),
        "revokedReason": device.revoked_reason,
        "stale": device.stale(now),
    })
}

/// GET /api/agent/devices
pub async fn list_devices(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let list = devices::list(&db, owner).await.map_err(store_failed)?;
    let parent = devices::parent(&db, owner).await.map_err(store_failed)?;
    let now = Utc::now();
    Ok(Json(json!({
        "devices": list.iter().map(|device| device_json(device, now)).collect::<Vec<_>>(),
        "parent": parent.unwrap_or_else(|| "site".to_string()),
        "limit": devices::MAX_DEVICES,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddDeviceRequest {
    name: String,
}

/// POST /api/agent/devices
pub async fn add_device(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<AddDeviceRequest>,
) -> Result<axum::response::Response, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "A device needs a name", "code": "device_name_required" })),
        )));
    }
    let (device, token) = match devices::add(&db, owner, name).await {
        Ok(added) => added,
        Err(AddError::Limit) => {
            return Err(HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": format!("A site can have up to {} devices", devices::MAX_DEVICES),
                    "code": "device_limit",
                    "limit": devices::MAX_DEVICES,
                })),
            )));
        }
        Err(AddError::Db(error)) => return Err(store_failed(error)),
    };
    tracing::info!(device = %device.id, "[sync] device added");
    Ok((
        StatusCode::CREATED,
        Json(json!({ "device": device_json(&device, Utc::now()), "token": token })),
    )
        .into_response())
}

/// DELETE /api/agent/devices/{device_id}
pub async fn remove_device(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(device_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let removed = devices::revoke(&db, owner, &device_id, RevokeReason::ByOwner)
        .await
        .map_err(store_failed)?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "No such device", "code": "device_not_found" })),
            ))
        })?;
    tracing::info!(device = %removed.id, "[sync] device removed by the owner");
    Ok(Json(json!({ "device": device_json(&removed, Utc::now()) })))
}
