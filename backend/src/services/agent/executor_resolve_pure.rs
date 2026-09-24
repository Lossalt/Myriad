use crate::services::agent::executor_utils_pure::truncate_str;

use crate::services::agent::types::{QuestionType, RiskLevel, UserQuestion};

use crate::services::agent::SYSTEM_USER_ID;

use chrono::{DateTime, Utc};

use serde_json::{Value, json};

use std::collections::HashMap;

/// 心跳（`SYSTEM_USER_ID`）无人值守时始终不能执行的能力：它们会创建、修改或
/// 触发新的自动执行，一次注入就能借心跳自我扩散。
pub const UNATTENDED_DENIED_CAPABILITIES: &[&str] = &[
    "heartbeat.create",
    "heartbeat.update",
    "heartbeat.delete",
    "heartbeat.toggle",
    "scheduler.create",
    "scheduler.trigger",
    "task.submit",
    "tapp.install",
];

/// 心跳能否不经人工确认执行一个需要确认的能力。
///
/// High / Critical 与 [`UNATTENDED_DENIED_CAPABILITIES`] 一律不行；其余 Low、Medium
/// 照常自动执行（例如 `http.fetch`）。
pub fn unattended_may_auto_run(capability_id: &str, risk: RiskLevel) -> bool {
    !matches!(risk, RiskLevel::High | RiskLevel::Critical)
        && !UNATTENDED_DENIED_CAPABILITIES.contains(&capability_id)
}

/// Whether an unconfirmed dynamically-generated step must be blocked.
///
/// Aligns with [`Agent::system_sensitive_gate`]: heartbeat follows
/// [`unattended_may_auto_run`]; interactive users block Medium and above until
/// confirmed.
pub fn should_block_unconfirmed_dynamic_step(
    user_id: i32,
    capability_id: &str,
    risk: RiskLevel,
) -> bool {
    if user_id == SYSTEM_USER_ID {
        !unattended_may_auto_run(capability_id, risk)
    } else {
        matches!(
            risk,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_risk_gate_aligns_with_system_sensitive_gate() {
        let gate = should_block_unconfirmed_dynamic_step;
        // Heartbeat: High+ and self-spreading capabilities blocked; Low and
        // Medium otherwise auto-run.
        assert!(gate(SYSTEM_USER_ID, "cache.clear", RiskLevel::High));
        assert!(gate(SYSTEM_USER_ID, "system.shutdown", RiskLevel::Critical));
        assert!(!gate(SYSTEM_USER_ID, "http.fetch", RiskLevel::Medium));
        assert!(!gate(SYSTEM_USER_ID, "storage.set", RiskLevel::Low));
        for id in UNATTENDED_DENIED_CAPABILITIES {
            assert!(gate(SYSTEM_USER_ID, id, RiskLevel::Medium), "{id}");
            assert!(gate(SYSTEM_USER_ID, id, RiskLevel::Low), "{id}");
        }
        // Interactive: Medium+ blocked until confirmed, whatever the capability.
        assert!(gate(7, "cache.clear", RiskLevel::High));
        assert!(gate(7, "http.fetch", RiskLevel::Medium));
        assert!(!gate(7, "storage.set", RiskLevel::Low));
        assert!(!gate(7, "heartbeat.toggle", RiskLevel::Low));
    }
}
