//! The site owner's devices that sync her: add one (its token is shown this
//! once), list them, remove one; and her host lease: where she lives, and a
//! move of her started, confirmed or called off. Session routes, owner only:
//! a move that writes to the site's her is confirmed here, logged in, never
//! by a device token alone.

use super::*;
use crate::error::HttpError;
use crate::services::persona_sync::devices::{self, AddError, Device, RevokeReason};
use crate::services::persona_sync::lease;
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
        "createdAt": device.created_at.to_rfc3339(),
        "lastSeenAt": device.last_seen_at.map(|at| at.to_rfc3339()),
        "revokedAt": device.revoked_at.map(|at| at.to_rfc3339()),
        "revokedReason": device.revoked_reason,
        "stale": device.stale(now),
        // Its link to her is open now (lastSeenAt is when it last was).
        "connected": device.revoked_at.is_none()
            && crate::services::agent::link::is_open(&device.id),
    })
}

/// The owner's devices, where she lives, and the limit.
async fn device_list(db: &DatabaseConnection, owner: i32) -> Result<Value, HttpError> {
    let list = devices::list(db, owner).await.map_err(store_failed)?;
    let host = lease::shown(db).await.map_err(store_failed)?;
    let now = Utc::now();
    // What came by chat app while she was away, held until she is back (D3).
    let held = crate::services::channel_work::held::count(db)
        .await
        .map_err(store_failed)?;
    Ok(json!({
        "devices": list.iter().map(|device| device_json(device, now)).collect::<Vec<_>>(),
        "host": host,
        "held": held,
        "limit": devices::MAX_DEVICES,
    }))
}

/// GET /api/agent/devices
pub async fn list_devices(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(device_list(&db, owner).await?))
}

/// GET /api/agent/host: where she lives, as the devices page shows it.
pub async fn get_host(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartMoveRequest {
    kind: String,
    device: String,
}

/// POST /api/agent/host/moves: the owner, logged in, starts a move of her
/// with one of their live devices (`move`: she goes to live on it; `merge`:
/// its her joins the site's). Asked here it is confirmed already: she is
/// frozen at once. Answers the lease.
pub async fn start_move(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<StartMoveRequest>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let Some(kind) = lease::Kind::parse(&request.kind) else {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "kind is move or merge", "code": "invalid_move_kind" })),
        )));
    };
    let live = devices::list(&db, owner)
        .await
        .map_err(store_failed)?
        .into_iter()
        .any(|device| device.id == request.device && device.revoked_at.is_none());
    if !live {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "No such device", "code": "device_not_found" })),
        )));
    }
    lease::start(&db, kind, &request.device, lease::Starter::Owner)
        .await
        .map_err(super::sync::move_refused)?;
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
}

/// POST /api/agent/host/moves/{move_id}/confirm: the owner, logged in,
/// confirms a move a device asked for. She is frozen where she is.
pub async fn confirm_move(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(move_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    lease::confirm(&db, &move_id)
        .await
        .map_err(super::sync::move_refused)?;
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
}

/// POST /api/agent/host/moves/{move_id}/cancel: the owner calls a move off
/// before its commit, or turns down one a device asked for: she is where she
/// was, as she was.
pub async fn cancel_move(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(move_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    lease::cancel(&db, &move_id, None)
        .await
        .map_err(super::sync::move_refused)?;
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
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

/// DELETE /api/agent/host/held: the owner lets go of what came by chat app
/// while she was away; she will not read it (D3-4).
pub async fn clear_held(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let cleared = crate::services::channel_work::held::clear(&db)
        .await
        .map_err(store_failed)?;
    Ok(Json(json!({ "cleared": cleared, "held": 0 })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    passphrase: String,
}

/// POST /api/agent/host/export: 「导出她」. The owner, logged in, gets
/// everything the site has of her as one `myriad.her-archive` file sealed
/// with the passphrase they typed (always sealed, H3-6). The passphrase is
/// used once for the key and kept nowhere. Only while she lives here or is
/// moving.
pub async fn export_her(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<ExportRequest>,
) -> Result<axum::response::Response, HttpError> {
    use crate::services::persona_sync::export::{self, ExportError};
    use axum::http::header;
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let refused = |status: StatusCode, error: &str, code: &str, extra: Option<(&str, Value)>| {
        let mut body = json!({ "error": error, "code": code });
        if let Some((key, value)) = extra {
            body[key] = value;
        }
        HttpError::from((status, Json(body)))
    };
    match export::export(&db, owner, request.passphrase).await {
        Ok(file) => Ok((
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", file.name),
                ),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            file.bytes,
        )
            .into_response()),
        Err(ExportError::Passphrase) => Err(refused(
            StatusCode::BAD_REQUEST,
            "An export is sealed with a passphrase",
            "passphrase_required",
            None,
        )),
        Err(ExportError::Elsewhere(device)) => {
            let name = match device {
                Some(id) => lease::device_name(&db, &id).await.ok().flatten(),
                None => None,
            };
            Err(refused(
                StatusCode::CONFLICT,
                "She does not live on this site now",
                "host_moved",
                Some((
                    "host",
                    json!(name.map(|name| json!({ "kind": "app", "name": name }))),
                )),
            ))
        }
        Err(ExportError::Empty) => Err(refused(
            StatusCode::NOT_FOUND,
            "The site has no her to export",
            "her_absent",
            None,
        )),
        Err(ExportError::Failed(what)) => {
            tracing::error!(what, "[sync] she could not be exported");
            Err(refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "She could not be exported",
                "export_failed",
                None,
            ))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(user_id: i32) -> Claims {
        Claims {
            sub: user_id.to_string(),
            username: format!("user-{user_id}"),
            is_admin: user_id == 41,
            is_owner: user_id == 41,
            exp: i64::MAX,
            iat: 0,
            tv: 0,
            subject: crate::middleware::auth::AuthSubject::from_test_sub(&user_id.to_string()),
        }
    }

    async fn answer(result: Result<Json<Value>, HttpError>) -> Result<Value, (StatusCode, Value)> {
        match result {
            Ok(Json(value)) => Ok(value),
            Err(error) => {
                let response = error.into_response();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
                    .await
                    .unwrap();
                Err((
                    status,
                    serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                ))
            }
        }
    }

    async fn start(
        db: &DatabaseConnection,
        user: i32,
        kind: &str,
        device: &str,
    ) -> Result<Value, (StatusCode, Value)> {
        let request: StartMoveRequest =
            serde_json::from_value(json!({ "kind": kind, "device": device })).unwrap();
        answer(start_move(State(db.clone()), Extension(claims(user)), Json(request)).await).await
    }

    /// Where she lives, and a move of her, from the owner's session (an
    /// isolated schema on a real database; skipped without a test database):
    /// the device list says where she lives; a move the owner starts freezes
    /// her and can be called off; one a device asked for waits for the
    /// owner's confirmation, which nobody else can give.
    #[tokio::test]
    async fn the_owner_moves_her_from_the_site() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = fixture.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES \
             (41, 'owner41', true, true), (42, 'guest42', false, false); \
             INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW())",
        )
        .await
        .unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let (mac, _) = devices::add(&db, 41, "Mac").await.unwrap();

        let list = answer(list_devices(State(db.clone()), Extension(claims(41))).await)
            .await
            .unwrap();
        assert_eq!(list["host"]["holder"], "site");
        assert_eq!(list["host"]["epoch"], 1);
        assert_eq!(list["host"]["state"], "live");
        assert!(list.get("parent").is_none());
        assert!(list["devices"][0].get("role").is_none());

        let (status, body) = start(&db, 41, "move", "dev_nope").await.unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("device_not_found"))
        );
        let (status, _) = start(&db, 42, "move", &mac.id).await.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        let frozen = start(&db, 41, "move", &mac.id).await.unwrap();
        assert_eq!(frozen["host"]["state"], "frozen");
        assert_eq!(frozen["host"]["move"]["phase"], "frozen");
        assert_eq!(frozen["host"]["move"]["toName"], "Mac");
        let id = frozen["host"]["move"]["id"].as_str().unwrap().to_string();
        let (status, body) = start(&db, 41, "merge", &mac.id).await.unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("move_in_progress"))
        );
        let back = answer(cancel_move(State(db.clone()), Extension(claims(41)), Path(id)).await)
            .await
            .unwrap();
        assert_eq!(back["host"]["state"], "live");
        assert_eq!(back["host"]["move"], Value::Null);
        assert_eq!(back["host"]["epoch"], 1);

        // Asked by the device: only the owner confirms it.
        let proposed = lease::start(&db, lease::Kind::Merge, &mac.id, lease::Starter::Device)
            .await
            .unwrap();
        let id = proposed.moving.unwrap().id;
        let (status, _) =
            answer(confirm_move(State(db.clone()), Extension(claims(42)), Path(id.clone())).await)
                .await
                .unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            lease::read(&db).await.unwrap().unwrap().state,
            lease::State::Live
        );
        let confirmed =
            answer(confirm_move(State(db.clone()), Extension(claims(41)), Path(id.clone())).await)
                .await
                .unwrap();
        assert_eq!(confirmed["host"]["state"], "frozen");
        assert_eq!(confirmed["host"]["move"]["by"], "device");
        let (status, body) =
            answer(confirm_move(State(db.clone()), Extension(claims(41)), Path(id.clone())).await)
                .await
                .unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("move_phase"))
        );
        answer(cancel_move(State(db.clone()), Extension(claims(41)), Path(id)).await)
            .await
            .unwrap();

        fixture.close().await;
    }
}
