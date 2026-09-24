//! The one resolution of an Agent step's granted permissions.
//!
//! Granted = the user's current role permissions, and for a turn running under
//! personal autonomy, the autonomy grant re-read now (a revocation takes effect
//! on the next step) intersected with the turn's cap. Every execution path
//! (executor steps, the work loop, web-owned capabilities) calls this instead
//! of combining the pieces itself. Declared and approved permissions are not
//! involved: this is the granted layer only.

use sea_orm::DatabaseConnection;

use super::{AutonomyGrantStore, autonomy_execute_permission_error, effective_granted_permissions};

/// Permissions this step may use now. Fails closed when the autonomy grant
/// cannot be read or no longer allows the cap.
pub async fn effective_granted(
    db: &DatabaseConnection,
    user_id: i32,
    autonomy_cap: Option<&[String]>,
) -> Result<Vec<String>, String> {
    let current: Vec<String> = crate::services::agent::get_user_permissions(db, user_id)
        .await
        .into_iter()
        .collect();
    if autonomy_cap.is_some() {
        let grant = AutonomyGrantStore::new(db.clone())
            .find(user_id)
            .await
            .map_err(|_| "Unable to verify autonomy grant".to_string())?;
        if autonomy_execute_permission_error(
            user_id,
            grant.as_ref(),
            &current,
            autonomy_cap,
            "",
            &[],
        )
        .is_some()
        {
            return Err("Personal autonomy is no longer granted".into());
        }
    }
    Ok(effective_granted_permissions(&current, autonomy_cap))
}

/// [`effective_granted`], then require every permission `capability_id` needs.
/// Returns the granted set for checks that depend on parameters.
pub async fn authorize_capability(
    db: &DatabaseConnection,
    user_id: i32,
    autonomy_cap: Option<&[String]>,
    capability_id: &str,
    required: &[String],
) -> Result<Vec<String>, String> {
    let granted = effective_granted(db, user_id, autonomy_cap).await?;
    missing_permission(&granted, capability_id, required).map_or(Ok(granted), Err)
}

pub fn missing_permission(
    granted: &[String],
    capability_id: &str,
    required: &[String],
) -> Option<String> {
    required
        .iter()
        .find(|permission| !granted.contains(permission))
        .map(|permission| format!("权限不足：执行 '{capability_id}' 需要 '{permission}' 权限"))
}

#[cfg(test)]
mod tests {
    use super::missing_permission;

    #[test]
    fn a_step_needs_every_required_permission_in_the_granted_set() {
        let granted = vec!["storage:read".to_string()];
        assert!(missing_permission(&granted, "x", &["storage:read".into()]).is_none());
        assert!(
            missing_permission(&granted, "x", &["storage:write".into()])
                .is_some_and(|message| message.contains("storage:write"))
        );
        assert!(missing_permission(&granted, "x", &[]).is_none());
    }
}
