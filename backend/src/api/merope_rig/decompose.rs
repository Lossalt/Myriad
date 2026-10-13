//! See-through 远程拆层的配置：令牌和 Space。拆层本身作为任务跑（turn_keyforms），
//! 刷新页面也能接上。默认用 Myriad 自己的 Space，它提供 `decompose`，立绘按自身
//! 比例放进画布拆；主人也可以换成别的 Space，没有 `decompose` 的（如官方演示）走正方形。

use axum::{Extension, Json, extract::State, http::StatusCode};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, internal_error, require_merope_enabled, require_owner};
use crate::{middleware::auth::Claims, services::see_through};

pub(super) fn see_through_error(error: see_through::SeeThroughError) -> ApiError {
    use see_through::SeeThroughError;

    if error.is_gpu_shortage() {
        tracing::warn!(%error, "See-through could not get a GPU");
        // The Space's own words (how much quota is left, when it renews) go to the owner.
        let detail = match &error {
            SeeThroughError::Rejected(Some(message)) => message.as_str(),
            _ => "See-through ZeroGPU is unavailable; check the Hugging Face token and quota",
        };
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": detail, "code": "see_through_quota_unavailable" })),
        );
    }
    let (status, code, message) = match &error {
        SeeThroughError::NotConfigured => (
            StatusCode::PRECONDITION_REQUIRED,
            "see_through_token_required",
            "Configure a Hugging Face API token before using See-through",
        ),
        SeeThroughError::Busy => (
            StatusCode::CONFLICT,
            "see_through_busy",
            "A See-through decomposition is already running",
        ),
        SeeThroughError::InvalidInput(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": message, "code": "see_through_invalid_input" })),
            );
        }
        SeeThroughError::Authentication => (
            StatusCode::BAD_GATEWAY,
            "see_through_auth_failed",
            "Hugging Face rejected the configured API token",
        ),
        SeeThroughError::Quota | SeeThroughError::Rejected(_) => {
            let message = match &error {
                SeeThroughError::Rejected(message) => message.clone(),
                _ => None,
            };
            tracing::warn!(?message, "See-through rejected a job");
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": message.as_deref().unwrap_or("See-through rejected the job without a reason"),
                    "code": "see_through_upstream_failed"
                })),
            );
        }
        SeeThroughError::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "see_through_timeout",
            "See-through inference timed out",
        ),
        SeeThroughError::Waking => (
            StatusCode::SERVICE_UNAVAILABLE,
            "see_through_space_waking",
            "The See-through Space is still starting; try again in a few minutes",
        ),
        SeeThroughError::SpaceUnavailable(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "see_through_space_unavailable",
            "The See-through Space is paused or failed; check it on Hugging Face",
        ),
        SeeThroughError::Transport(_)
        | SeeThroughError::Upstream { .. }
        | SeeThroughError::InvalidOutput(_) => (
            StatusCode::BAD_GATEWAY,
            "see_through_upstream_failed",
            "See-through returned an invalid or unavailable result",
        ),
    };
    tracing::warn!(%error, code, "remote See-through request failed");
    (status, Json(json!({ "error": message, "code": code })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSeeThroughTokenRequest {
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSeeThroughSpaceRequest {
    /// `owner/name`; empty or absent goes back to Myriad's own Space.
    #[serde(default)]
    space: Option<String>,
}

fn space_status(space: &see_through::Space) -> Value {
    json!({
        "provider": space.name(),
        "space": space.name(),
        "defaultSpace": see_through::DEFAULT_SPACE,
    })
}

pub async fn get_see_through_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let (token_configured, space) = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        (
            see_through::configured_hf_token(&config).is_some(),
            see_through::configured_space(&config),
        )
    };
    let mut status = space_status(&space);
    status["tokenConfigured"] = json!(token_configured);
    status["defaultResolution"] = json!(see_through::DecomposeOptions::default().resolution);
    status["splitArmsAndLegs"] = json!(true);
    Ok(Json(status))
}

pub async fn update_see_through_token(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<UpdateSeeThroughTokenRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let token = see_through::validate_hf_token(&payload.token).map_err(see_through_error)?;
    crate::services::config_service::ConfigService::new(db)
        .update_config("see_through_hf_token", json!(token.clone()))
        .await
        .map_err(internal_error)?;
    crate::GLOBAL_DYNAMIC_CONFIG
        .write()
        .await
        .see_through_hf_token = Some(token);
    let space = see_through::configured_space(&*crate::GLOBAL_DYNAMIC_CONFIG.read().await);
    let mut status = space_status(&space);
    status["tokenConfigured"] = json!(true);
    Ok(Json(status))
}

/// Names the Space to decompose with, or none for Myriad's own.
pub async fn update_see_through_space(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<UpdateSeeThroughSpaceRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let space = payload
        .space
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(see_through::Space::parse)
        .transpose()
        .map_err(see_through_error)?;
    let stored = space.as_ref().map(|space| space.name().to_string());
    crate::services::config_service::ConfigService::new(db)
        .update_config(
            "see_through_space",
            json!(stored.clone().unwrap_or_default()),
        )
        .await
        .map_err(internal_error)?;
    let (token_configured, space) = {
        let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
        config.see_through_space = stored;
        (
            see_through::configured_hf_token(&config).is_some(),
            see_through::configured_space(&config),
        )
    };
    let mut status = space_status(&space);
    status["tokenConfigured"] = json!(token_configured);
    Ok(Json(status))
}
