//! 补找会自己动的部件：对已有的装配包（穿着的半身，或某套全身）在后台重跑一次
//! 部件分析，完成后另存为新包换上。用于这项分析出现之前导入的包，或上次没找成的。

use std::{collections::BTreeMap, sync::Mutex, time::Instant};

use axum::{
    Extension, Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::{
    ApiResult, bad_request,
    full_body::full_body_set,
    import::{MAX_RIG_ANALYSIS_REFERENCE_BYTES, find_moving_parts, png_validation_error},
    internal_error,
    master::{MasterSlot, master_for, master_from_persona},
    not_found,
    package::{rig_revision_conflict, rig_revision_matches},
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{agent::merope, merope_rig},
};

/// The searches running, by slot (none: the worn bust; a set's id: that
/// full-body set), with when each began: a page opened while one runs shows it.
static SEARCHES: Mutex<BTreeMap<Option<String>, Instant>> = Mutex::new(BTreeMap::new());

/// A slot's search under way, given up when it ends, however it ends.
pub(super) struct PartSearch(Option<String>);

impl PartSearch {
    /// The slot's search, unless one is running for it already.
    pub(super) fn begin(outfit: Option<String>) -> Option<Self> {
        let mut searches = SEARCHES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if searches.contains_key(&outfit) {
            return None;
        }
        searches.insert(outfit.clone(), Instant::now());
        Some(Self(outfit))
    }
}

impl Drop for PartSearch {
    fn drop(&mut self) {
        SEARCHES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.0);
    }
}

/// The searches running now: the bust's (`outfitId` null) and any full-body set's.
pub async fn running_part_searches(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let searches = SEARCHES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let running: Vec<Value> = searches
        .iter()
        .map(|(outfit, began)| json!({ "outfitId": outfit, "seconds": began.elapsed().as_secs() }))
        .collect();
    Ok(Json(json!({ "searches": running })))
}

/// One search already running for the slot: this request joins it.
fn already_running() -> (StatusCode, Json<Value>) {
    (
        StatusCode::ACCEPTED,
        Json(json!({ "started": false, "running": true })),
    )
}

/// The worn bust's package: its moving parts looked for again.
pub async fn find_site_rig_parts(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let (previous, reference) = read_request(multipart).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    if !rig_revision_matches(
        persona.as_ref().and_then(|row| row.visual_profile.as_ref()),
        Some(&previous),
    ) {
        return Err(rig_revision_conflict());
    }
    let master = persona
        .as_ref()
        .and_then(master_from_persona)
        .ok_or_else(|| not_found("Site portrait is missing"))?;
    let Some(search) = PartSearch::begin(None) else {
        return Ok(already_running());
    };
    tokio::spawn(find_moving_parts(
        search, db, user_id, None, previous, master, reference,
    ));
    Ok((StatusCode::ACCEPTED, Json(json!({ "started": true }))))
}

/// A full-body set's package: its moving parts looked for again.
pub async fn find_full_body_parts(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let (previous, reference) = read_request(multipart).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let set = full_body_set(persona.as_ref(), &outfit_id)?;
    if set.rig_asset_id.as_deref() != Some(previous.as_str()) {
        return Err(rig_revision_conflict());
    }
    let master = persona
        .as_ref()
        .and_then(|row| master_for(row, MasterSlot::FullBody(&outfit_id)))
        .ok_or_else(|| not_found("The full-body set has no picture"))?;
    let Some(search) = PartSearch::begin(Some(outfit_id.clone())) else {
        return Ok(already_running());
    };
    tokio::spawn(find_moving_parts(
        search,
        db,
        user_id,
        Some(outfit_id),
        previous,
        master,
        reference,
    ));
    Ok((StatusCode::ACCEPTED, Json(json!({ "started": true }))))
}

/// The package the page shows (`assetId`) and that package drawn at rest as
/// the page composes it (`analysisReference`, PNG): what the vision model looks at.
async fn read_request(mut multipart: Multipart) -> ApiResult<(String, Vec<u8>)> {
    let mut asset_id = None;
    let mut reference = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| bad_request("Invalid part analysis request"))?
    {
        match field.name() {
            Some("assetId") if asset_id.is_none() => {
                let text = field
                    .text()
                    .await
                    .map_err(|_| bad_request("Invalid part analysis request"))?;
                asset_id = Some(
                    merope_rig::normalize_asset_id(&text)
                        .ok_or_else(|| bad_request("Invalid rig asset id"))?,
                );
            }
            Some("analysisReference") if reference.is_none() => {
                if field.content_type() != Some("image/png") {
                    return Err(bad_request("Rig analysis reference must be a PNG image"));
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| bad_request("Invalid part analysis request"))?;
                if bytes.len() > MAX_RIG_ANALYSIS_REFERENCE_BYTES {
                    return Err(bad_request("Rig analysis reference exceeds 10 MB"));
                }
                reference = Some(bytes.to_vec());
            }
            _ => {
                return Err(bad_request(
                    "Part analysis request has an unsupported field",
                ));
            }
        }
    }
    let asset_id = asset_id.ok_or_else(|| bad_request("Part analysis request has no asset id"))?;
    let reference =
        reference.ok_or_else(|| bad_request("Part analysis request has no reference"))?;
    let (reference, _) = merope_rig::validate_png_dimensions(reference)
        .await
        .map_err(png_validation_error)?;
    Ok((asset_id, reference))
}
