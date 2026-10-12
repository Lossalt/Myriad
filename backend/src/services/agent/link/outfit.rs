//! The set she wears on the app (D7): each end has its own worn set, kept on
//! her host in her visual profile as `activeOutfitByEnd["app:<deviceId>"]`,
//! apart from the site's own bust and full-body sets. A child that takes
//! `wardrobe` is told it (`outfit`: the set and its rig's content id, which it
//! fetches through `/api/agent/sync/assets/{id}`), and can change it
//! (`outfit.wear`). Changing what the site wears does not change the app's,
//! and the other way round.
//!
//! An app wears only a set with a rig. The first time it connects without a
//! set of its own, it gets the one the site wears full-body (else its bust,
//! else the first set with a rig), and keeps it from then on.

use sea_orm::{DatabaseConnection, EntityTrait, QuerySelect, TransactionTrait};
use serde_json::{Map, Value, json};

use crate::services::agent::merope;

use super::connection::Outgoing;

pub(super) const BY_END: &str = "activeOutfitByEnd";

/// What an app wears: the set, and its rig's (and picture's) content ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Worn {
    pub(super) outfit_id: String,
    pub(super) rig_asset_id: String,
    pub(super) portrait_asset_id: Option<String>,
}

fn end_key(device_id: &str) -> String {
    format!("app:{device_id}")
}

/// The set `id` of her wardrobe, when it has a rig.
fn wearable(profile: &Value, id: &str) -> Option<Worn> {
    let item = profile
        .get("wardrobe")?
        .as_array()?
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(id))?;
    Some(Worn {
        outfit_id: id.to_string(),
        rig_asset_id: item.get("rigAssetId")?.as_str()?.to_string(),
        portrait_asset_id: item
            .get("portraitAssetId")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// What the app `device_id` wears, and whether that is its own (false: the
/// set it starts with).
pub(super) fn worn(profile: &Value, device_id: &str) -> Option<(Worn, bool)> {
    if let Some(own) = profile
        .get(BY_END)
        .and_then(|ends| ends.get(end_key(device_id)))
        .and_then(Value::as_str)
        .and_then(|id| wearable(profile, id))
    {
        return Some((own, true));
    }
    let started = ["activeFullBodyOutfitId", "activeOutfitId"]
        .iter()
        .filter_map(|key| profile.get(*key).and_then(Value::as_str))
        .find_map(|id| wearable(profile, id))
        .or_else(|| {
            profile
                .get("wardrobe")?
                .as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .find_map(|id| wearable(profile, id))
        })?;
    Some((started, false))
}

/// `profile` with the app `device_id` wearing `outfit_id`.
pub(super) fn wearing(profile: &Value, device_id: &str, outfit_id: &str) -> Value {
    let mut profile = profile.clone();
    if let Some(root) = profile.as_object_mut() {
        let ends = root
            .entry(BY_END.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !ends.is_object() {
            *ends = Value::Object(Map::new());
        }
        ends.as_object_mut()
            .expect("an object")
            .insert(end_key(device_id), json!(outfit_id));
    }
    profile
}

/// The set is not one the app can wear.
#[derive(Debug)]
pub(super) struct Unknown;

/// Writes what the app wears into her visual profile, under the persona
/// row's lock: `outfit_id`, or (`None`) the set it starts with when it has
/// none of its own yet. What it wears now.
pub(super) async fn put(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    outfit_id: Option<&str>,
) -> Result<Option<Worn>, Unknown> {
    let Ok(transaction) = db.begin().await else {
        return Ok(None);
    };
    let row = crate::models::entities::agent_persona::Entity::find_by_id(
        merope::api::store::PERSONA_ROW_ID,
    )
    .lock_exclusive()
    .one(&transaction)
    .await
    .ok()
    .flatten();
    let Some(row) = row else {
        return Ok(None);
    };
    let profile = row.visual_profile.clone().unwrap_or_else(|| json!({}));
    let next = match outfit_id {
        Some(id) => wearable(&profile, id).ok_or(Unknown)?,
        None => match worn(&profile, device_id) {
            Some((worn, true)) => return Ok(Some(worn)),
            Some((worn, false)) => worn,
            None => return Ok(None),
        },
    };
    let profile = wearing(&profile, device_id, &next.outfit_id);
    let written = merope::upsert_persona_on(
        &transaction,
        row.name,
        row.personality,
        merope::PortraitUpdate::Keep,
        merope::PersonaContractUpdate {
            visual_profile: merope::JsonDocumentUpdate::Set(profile),
            ..Default::default()
        },
        owner,
    )
    .await;
    match written {
        Ok(_) => Ok(transaction.commit().await.ok().map(|()| next)),
        Err(error) => {
            tracing::warn!(%error, device = %device_id, "[link] could not keep what the app wears");
            let _ = transaction.rollback().await;
            Ok(None)
        }
    }
}

/// What the app wears now, read only.
pub(super) async fn current(db: &DatabaseConnection, device_id: &str) -> Option<Worn> {
    let row = merope::get_persona(db).await.ok().flatten()?;
    worn(row.visual_profile.as_ref()?, device_id).map(|(worn, _)| worn)
}

/// Her wardrobe as a child shows it, read only (`wardrobe`): every set in
/// her order, whether it can be worn on an app (it has a rig), and the
/// content ids of its rig and picture.
pub(super) fn listed(profile: &Value) -> Vec<Value> {
    let Some(items) = profile.get("wardrobe").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str().filter(|id| !id.is_empty())?;
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);
            let profile = match item.get("profile").and_then(Value::as_str) {
                Some("fullBody") => "fullBody",
                _ => "bust",
            };
            let mut listed = json!({
                "outfitId": id,
                "name": name,
                "profile": profile,
                // As `wearable`: a set with a rig.
                "wearable": item.get("rigAssetId").and_then(Value::as_str).is_some(),
            });
            for key in ["rigAssetId", "portraitAssetId"] {
                if let Some(asset) = item
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                {
                    listed[key] = asset.into();
                }
            }
            Some(listed)
        })
        .collect()
}

/// The `wardrobe` frame.
pub(super) fn list_frame(outfits: Vec<Value>) -> Outgoing {
    Outgoing::latest("wardrobe", json!({ "outfits": outfits }))
}

/// Her wardrobe as listed now, read only.
pub(super) async fn current_list(db: &DatabaseConnection) -> Vec<Value> {
    merope::get_persona(db)
        .await
        .ok()
        .flatten()
        .and_then(|row| row.visual_profile)
        .map(|profile| listed(&profile))
        .unwrap_or_default()
}

/// The `outfit` frame.
pub(super) fn frame(worn: &Worn) -> Outgoing {
    let mut body = json!({
        "outfitId": worn.outfit_id,
        "rigAssetId": worn.rig_asset_id,
    });
    if let Some(portrait) = &worn.portrait_asset_id {
        body["portraitAssetId"] = portrait.clone().into();
    }
    Outgoing::latest("outfit", body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Value {
        json!({
            "activeOutfitId": "default",
            "activeFullBodyOutfitId": "coat",
            "wardrobe": [
                { "id": "default", "rigAssetId": "aa".repeat(32), "portraitAssetId": "p1" },
                { "id": "coat", "profile": "fullBody", "rigAssetId": "bb".repeat(32) },
                { "id": "plain" },
            ],
        })
    }

    #[test]
    fn each_app_wears_its_own_set_and_starts_with_the_sites_full_body() {
        let profile = profile();
        let (started, own) = worn(&profile, "dev_a").unwrap();
        assert!(!own);
        assert_eq!(started.outfit_id, "coat");
        assert_eq!(started.rig_asset_id, "bb".repeat(32));
        let worn_a = wearing(&profile, "dev_a", "default");
        let (a, own) = worn(&worn_a, "dev_a").unwrap();
        assert!(own);
        assert_eq!(
            (a.outfit_id.as_str(), a.portrait_asset_id.as_deref()),
            ("default", Some("p1"))
        );
        // Another app, and the site's own sets, are not moved.
        assert_eq!(worn(&worn_a, "dev_b").unwrap().0.outfit_id, "coat");
        assert_eq!(worn_a["activeOutfitId"], "default");
        assert_eq!(worn_a["activeFullBodyOutfitId"], "coat");
        // A set without a rig is not worn on an app.
        assert!(wearable(&profile, "plain").is_none());
        let stale = wearing(&profile, "dev_a", "plain");
        assert_eq!(worn(&stale, "dev_a").unwrap(), (started, false));
        assert!(worn(&json!({ "wardrobe": [{ "id": "plain" }] }), "dev_a").is_none());
        assert_eq!(frame(&a).body["rigAssetId"], "aa".repeat(32));
    }
}
