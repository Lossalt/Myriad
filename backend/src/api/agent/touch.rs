//! Touch on her avatar over HTTP: who may, and the face a page reports.
//! What the touch is, how often, and how she reacts is `merope::touch`,
//! which her link uses for the app too.
use super::*;
use crate::error::HttpError;
use crate::services::agent::merope::api::touch::{Invalid, TouchSummary, appraise_ongoing, ended};

fn invalid(_: Invalid) -> HttpError {
    HttpError::from((
        StatusCode::BAD_REQUEST,
        Json(AppError::public_json("Invalid touch summary")),
    ))
}

/// Completion is a separate ingest event (`agent.merope.touch`), not a continuation of appraise.
pub async fn complete(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<TouchSummary>,
) -> Result<StatusCode, HttpError> {
    // Read only while she is not on the site (D5).
    crate::services::persona_sync::lease::refuse_edit_while_away()?;
    let user_id = super::parse_user_id_with_agent_access(&claims, &db).await?;
    let live = crate::services::agent::consciousness::last_live_presence(user_id);
    ended(user_id, &body, &live, None).await.map_err(invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn appraise(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<TouchSummary>,
) -> Result<Json<Value>, HttpError> {
    // Read only while she is not on the site (D5).
    crate::services::persona_sync::lease::refuse_edit_while_away()?;
    let user_id = super::parse_user_id_with_agent_access(&claims, &db).await?;
    let decision = appraise_ongoing(&db, user_id, &body)
        .await
        .map_err(invalid)?;
    Ok(decision
        .map(|d| Json(json!(d)))
        .unwrap_or_else(|| Json(json!({ "reaction": null }))))
}
use myriad_error::AppError;
