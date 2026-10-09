//! Her data across the site and this platform (`myriad_persona_sync`): the
//! one byte form of an entity, the comparison of two of her, the clock and
//! a rig's id. The format's types are the crate's; these calls only carry
//! them across.

use base64::Engine as _;
use myriad_merope::RigManifest;
use myriad_persona_sync::{
    EntityVersion, Holdings, Rev, canonicalize as canonical_entity, compare as compare_sides, hlc,
    is_empty as holds_nothing, rig_package_id as package_id,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::input;
use crate::Failure;

fn refused(error: impl std::fmt::Display) -> Failure {
    Failure::BadInput(error.to_string())
}

fn rev(raw: &str) -> Result<Rev, Failure> {
    raw.parse().map_err(refused)
}

/// Rough entities in, each in its one form out: the entity as JSON and its
/// canonical bytes as a string, which is what a platform stores and sends.
pub(super) fn canonicalize(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        entities: Vec<EntityVersion>,
    }
    let req: In = input(raw)?;
    let entities = req
        .entities
        .into_iter()
        .map(|entity| {
            let entity = canonical_entity(entity).map_err(refused)?;
            let canonical = String::from_utf8(entity.canonical_bytes())
                .map_err(|_| refused("canonical bytes are UTF-8"))?;
            Ok(json!({ "entity": entity, "canonical": canonical }))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({ "entities": entities }))
}

pub(super) fn compare(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        local: Vec<EntityVersion>,
        remote: Vec<EntityVersion>,
    }
    let req: In = input(raw)?;
    let comparison = compare_sides(req.local, req.remote).map_err(refused)?;
    serde_json::to_value(comparison).map_err(refused)
}

pub(super) fn is_empty(raw: &[u8]) -> Result<Value, Failure> {
    let holdings: Holdings = input(raw)?;
    Ok(json!({ "empty": holds_nothing(&holdings) }))
}

pub(super) fn hlc_next(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        last: Option<String>,
        now: i64,
        node: String,
    }
    let req: In = input(raw)?;
    let last = req.last.as_deref().map(rev).transpose()?;
    let next = hlc::next(last.as_ref(), req.now, &req.node).map_err(refused)?;
    Ok(json!({ "rev": next.to_string() }))
}

pub(super) fn hlc_observe(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        last: Option<String>,
        seen: String,
        now: i64,
        node: String,
    }
    let req: In = input(raw)?;
    let last = req.last.as_deref().map(rev).transpose()?;
    let seen = rev(&req.seen)?;
    let after = hlc::observe(last.as_ref(), &seen, req.now, &req.node).map_err(refused)?;
    Ok(json!({ "last": after.to_string() }))
}

pub(super) fn legacy_rev(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        updated_at: i64,
        #[serde(default)]
        origin: Option<String>,
    }
    let req: In = input(raw)?;
    let legacy = hlc::legacy(req.updated_at, req.origin.as_deref()).map_err(refused)?;
    Ok(json!({ "rev": legacy.to_string() }))
}

pub(super) fn rig_package_id(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// The atlas file's bytes, base64.
        atlas: String,
        manifest: RigManifest,
    }
    let req: In = input(raw)?;
    let atlas = base64::engine::general_purpose::STANDARD
        .decode(req.atlas.as_bytes())
        .map_err(|_| refused("atlas is not base64"))?;
    let id = package_id(&atlas, &req.manifest).map_err(refused)?;
    Ok(json!({ "id": id }))
}
