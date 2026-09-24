use crate::services::agent::executor_utils_pure::truncate_str;

use crate::services::agent::types::{QuestionType, RiskLevel, UserQuestion};

use crate::services::agent::SYSTEM_USER_ID;

use chrono::{DateTime, Utc};

use serde_json::{json, Value};

use std::collections::HashMap;

/// Whether an unconfirmed dynamically-generated step must be blocked.
///
/// Aligns with [`Agent::system_sensitive_gate`]: Medium and above block for every
/// identity, including heartbeat (`SYSTEM_USER_ID`). Low may still auto-run.
/// `user_id` stays so callers and the plan-time gate keep one shape.
pub fn should_block_unconfirmed_dynamic_step(user_id: i32, risk: RiskLevel) -> bool {
    let _ = user_id;
    matches!(
        risk,
        RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_risk_gate_aligns_with_system_sensitive_gate() {
        // Heartbeat: Medium and above blocked; Low still auto-runs.
        assert!(should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Critical
        ));
        assert!(should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::High
        ));
        assert!(should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Medium
        ));
        assert!(!should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Low
        ));
        // Interactive: Medium+ blocked until confirmed.
        assert!(should_block_unconfirmed_dynamic_step(7, RiskLevel::High));
        assert!(should_block_unconfirmed_dynamic_step(7, RiskLevel::Medium));
        assert!(!should_block_unconfirmed_dynamic_step(7, RiskLevel::Low));
    }
}
