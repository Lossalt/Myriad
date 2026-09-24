//! Grant checks shared by native tool calls and saved Recipes.
use super::capability::{capability_covered_by_grants, get_capabilities_by_ids};
use crate::services::agent::capability::CapabilityRef;
use std::collections::{HashMap, HashSet};

pub(crate) async fn capability_allowed_for_grants(
    capability_id: &str,
    params: &HashMap<String, serde_json::Value>,
    granted: &HashSet<String>,
) -> Result<(), String> {
    if let Some(skill_id) = CapabilityRef::parse(&capability_id).skill_id() {
        let allowed = match super::skill::get_skill_registry() {
            Some(registry) => match registry.get(skill_id).await {
                Some(skill) => super::skill::skill_covered_by_grants(&skill, Some(granted)).await,
                None => false,
            },
            None => false,
        };
        if !allowed {
            return Err(format!("capability '{capability_id}' is not available"));
        }
        return Ok(());
    }
    if CapabilityRef::parse(&capability_id).is_mcp() {
        if !granted.contains("mcp:execute") {
            return Err(format!("capability '{capability_id}' is not available"));
        }
        return Ok(());
    }
    match get_capabilities_by_ids(&[capability_id.to_string()])
        .await
        .into_iter()
        .next()
    {
        Some(cap) => {
            if !capability_covered_by_grants(&cap, Some(granted)) {
                return Err(format!("capability '{capability_id}' is not available"));
            }
        }
        None => {
            return Err(format!("capability '{capability_id}' is not available"));
        }
    }
    if capability_id == "scheduler.create" {
        crate::services::agent::scheduler_create_actions_within_grants(params, granted)?;
    }
    Ok(())
}
