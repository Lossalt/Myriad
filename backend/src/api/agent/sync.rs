//! What a device calls to sync her (`/api/agent/sync/*`), behind
//! `sync_device_auth`: who it is and what the site has (`hello`), leaving,
//! and which end her own life happens at.

use super::*;
use crate::error::HttpError;
use crate::middleware::sync_device::SyncDevice;
use crate::services::persona_sync::{self, devices};
use myriad_error::AppError;

pub(crate) fn sync_failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, "[sync] request failed");
    HttpError(AppError::internal("Sync failed"))
}

/// The site's owner, who every device belongs to (the gate already checked).
pub(crate) fn owner_of(device: &SyncDevice) -> i32 {
    device.0.user_id
}

/// GET /api/agent/sync/hello
pub async fn sync_hello(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
) -> Result<Json<Value>, HttpError> {
    let owner = owner_of(&device);
    let holdings = persona_sync::holdings(&db, owner)
        .await
        .map_err(sync_failed)?;
    let name = persona_sync::persona_name(&db).await.map_err(sync_failed)?;
    let embedding_model = persona_sync::embedding_model(&db)
        .await
        .map_err(sync_failed)?;
    let parent = devices::parent(&db, owner).await.map_err(sync_failed)?;
    Ok(Json(json!({
        "formatVersion": myriad_persona_sync::FORMAT_VERSION,
        "deviceId": device.0.id,
        "lineage": device.0.lineage,
        "empty": myriad_persona_sync::is_empty(&holdings),
        "persona": name.map(|name| json!({ "name": name })),
        "embeddingModel": embedding_model,
        "role": device.0.role.as_str(),
        "parent": parent.unwrap_or_else(|| "site".to_string()),
        "limits": {
            "pullMaxEntities": persona_sync::PULL_MAX_ENTITIES,
            "pullMaxBytes": persona_sync::PULL_MAX_BYTES,
            "pushMaxEntities": persona_sync::PUSH_MAX_ENTITIES,
            "pushMaxBytes": persona_sync::PUSH_MAX_BYTES,
            "assetMaxBytes": persona_sync::ASSET_MAX_BYTES,
        },
        "serverTime": Utc::now().timestamp_micros(),
    })))
}

/// POST /api/agent/sync/leave: the device gives its token back.
pub async fn sync_leave(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
) -> Result<Json<Value>, HttpError> {
    devices::revoke(
        &db,
        owner_of(&device),
        &device.0.id,
        devices::RevokeReason::DeviceLeft,
    )
    .await
    .map_err(sync_failed)?;
    tracing::info!(device = %device.0.id, "[sync] device left");
    Ok(Json(json!({ "left": true })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRequest {
    role: String,
}

/// PUT /api/agent/sync/role: chosen when the device connects. `parent`
/// makes this device the parent end (another parent device becomes a
/// child); `child` leaves the site as the parent unless another device is.
pub async fn sync_role(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Json(request): Json<RoleRequest>,
) -> Result<Json<Value>, HttpError> {
    let Some(role) = devices::Role::parse(&request.role) else {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "role is parent or child", "code": "invalid_role" })),
        )));
    };
    let owner = owner_of(&device);
    let updated = devices::set_role(&db, owner, &device.0.id, role)
        .await
        .map_err(sync_failed)?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "This device was removed from the site", "code": "device_revoked" })),
            ))
        })?;
    let parent = devices::parent(&db, owner).await.map_err(sync_failed)?;
    tracing::info!(device = %updated.id, role = role.as_str(), "[sync] device role set");
    Ok(Json(json!({
        "role": updated.role.as_str(),
        "parent": parent.unwrap_or_else(|| "site".to_string()),
    })))
}
