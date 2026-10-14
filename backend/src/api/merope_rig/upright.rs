//! 上传的立绘扶正：看一眼它侧没侧、有没有背景、手里拿没拿东西；需要时照它
//! 用它自己的画风重画一张正面的，连同「像不像」的检查交给站长定。重画的图不
//! 落库，站长选用时由页面照上传那样传回原来的位置。

use axum::{Extension, Json, extract::State, http::StatusCode};
use base64::Engine;
use myriad_merope::{CharacterAssetProfile, build_upright_portrait_prompt};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use super::{
    ApiResult, full_body::full_body_set, internal_error, not_found,
    portrait_generation_config_error, portrait_generation_provider_error, require_merope_enabled,
    require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{
        agent::merope::{self, api::onboarding_ai},
        image_generation,
    },
};

/// One redraw at a time: each is paid for.
static UPRIGHT_SLOT: Semaphore = Semaphore::const_new(1);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UprightRequest {
    /// The wardrobe set whose picture it is; none for the worn bust (the master portrait).
    #[serde(default)]
    outfit_id: Option<String>,
    /// The interface language, for the notes on what a redraw changed.
    #[serde(default)]
    language: Option<String>,
}

/// POST /api/merope/rig/portrait/framing
pub async fn read_portrait_framing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<UprightRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let (url, profile) = slot_portrait(&db, request.outfit_id.as_deref()).await?;
    let image = image_generation::load_local_reference(&url)
        .await
        .map_err(portrait_generation_provider_error)?;
    let framing = onboarding_ai::read_portrait_framing(&image)
        .await
        .map_err(vision_error)?;
    Ok(Json(json!({
        "portraitUrl": url,
        "profile": profile_name(profile),
        "facing": framing.facing,
        "backdrop": framing.backdrop,
        "arms": framing.arms,
        "holding": framing.holding,
        "reasons": framing.reasons,
    })))
}

/// POST /api/merope/rig/portrait/upright
pub async fn draw_upright_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<UprightRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let _slot = UPRIGHT_SLOT.try_acquire().map_err(|_| {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "A portrait is already being redrawn",
                "code": "upright_in_progress"
            })),
        )
    })?;
    let (url, profile) = slot_portrait(&db, request.outfit_id.as_deref()).await?;
    let original = image_generation::load_local_reference(&url)
        .await
        .map_err(portrait_generation_provider_error)?;
    let contract = profile.contract();
    let prompt = build_upright_portrait_prompt(profile);
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    tracing::info!(
        provider = %config.provider,
        model = %config.model,
        profile = profile_name(profile),
        "upright portrait redraw started"
    );
    let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "upright",
        image_generation::generate_image_with_background(
            &config,
            &prompt,
            contract.generation_width,
            contract.generation_height,
            Some(&original),
            Some(image_generation::ImageBackground::Opaque),
        ),
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    let (bytes, media_type) = image_generation::load_generated_bytes(generated)
        .await
        .map_err(portrait_generation_provider_error)?;
    // The redraw stands without its check: the owner sees both pictures either way.
    let language = request.language.as_deref().unwrap_or("en-US");
    let check = match image_generation::ImageReference::new(bytes.clone(), &media_type) {
        Ok(redraw) => onboarding_ai::check_upright_redraw(language, &original, &redraw)
            .await
            .map_err(|error| tracing::warn!(%error, "upright redraw check failed"))
            .ok(),
        Err(_) => None,
    };
    Ok(Json(json!({
        "sourceUrl": url,
        "profile": profile_name(profile),
        "image": base64::engine::general_purpose::STANDARD.encode(&bytes),
        "mediaType": media_type,
        "check": check.map(|check| json!({
            "sameCharacter": check.same_character,
            "sameStyle": check.same_style,
            "upright": check.upright,
            "differences": check.differences,
        })),
    })))
}

/// The picture a request is about: the worn bust's (the master portrait) or a
/// wardrobe set's, with the set's kind.
async fn slot_portrait(
    db: &DatabaseConnection,
    outfit_id: Option<&str>,
) -> ApiResult<(String, CharacterAssetProfile)> {
    let persona = merope::get_persona(db).await.map_err(internal_error)?;
    let found = match outfit_id.map(str::trim).filter(|id| !id.is_empty()) {
        None => persona
            .as_ref()
            .and_then(|row| row.portrait_asset_id.clone())
            .filter(|url| !url.is_empty())
            .map(|url| (url, CharacterAssetProfile::Bust)),
        Some(id) => {
            let profile = persona.as_ref().and_then(|row| row.visual_profile.as_ref());
            match full_body_set(persona.as_ref(), id) {
                Ok(set) => set
                    .portrait_asset_id
                    .map(|url| (url, CharacterAssetProfile::FullBody)),
                Err(_) => myriad_merope::wardrobe_bust_portrait(profile, id)
                    .map(|(url, _)| (url, CharacterAssetProfile::Bust)),
            }
        }
    };
    found.ok_or_else(|| not_found("This outfit has no portrait yet"))
}

fn profile_name(profile: CharacterAssetProfile) -> &'static str {
    match profile {
        CharacterAssetProfile::Bust => "bust",
        CharacterAssetProfile::FullBody => "fullBody",
    }
}

fn vision_error(error: onboarding_ai::OnboardingAiError) -> super::ApiError {
    tracing::error!(%error, "portrait framing read failed");
    match error {
        onboarding_ai::OnboardingAiError::AnalyzerUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Pro model is unavailable", "code": "pro_unavailable" })),
        ),
        other => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": other.public_detail().unwrap_or("The portrait could not be read"),
                "code": "portrait_framing_failed"
            })),
        ),
    }
}
