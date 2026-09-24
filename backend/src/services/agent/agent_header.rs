use once_cell::sync::Lazy;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::*;

/// 全局 Lane Queue（控制并发和会话级串行；无 session 时退回 `user:{id}`）
pub static LANE_QUEUE: Lazy<Arc<super::queue::LaneQueue>> =
    Lazy::new(|| Arc::new(super::queue::LaneQueue::new(4)));

/// 系统用户 ID（Heartbeat 定时任务等无人值守场景）
pub const SYSTEM_USER_ID: i32 = 0;

/// 待确认配方热缓存（持久化在 tapp_registry `agent_recipe_confirmation`）
pub(crate) static PENDING_CONFIRMATIONS: Lazy<
    Arc<RwLock<HashMap<String, PendingRecipeConfirmation>>>,
> = Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// 待确认的配方信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PendingRecipeConfirmation {
    /// 确认请求
    pub request: ConfirmationRequest,
    /// 原始配方
    pub recipe: Recipe,
    /// 用户 ID
    pub user_id: i32,
    /// 原始 PlannerOutput（确认后续 `generate_response_message_v2`；升级重规划不读此字段）
    pub planner_output: PlannerOutput,
    /// 发起确认时的会话 ID（确认续跑需写回同一 session 历史）
    #[serde(default)]
    pub session_id: Option<String>,
    /// 发起确认时的 run id（确认续跑复用同一 run hub / 通知）
    #[serde(default)]
    pub run_id: Option<String>,
    /// Consciousness proposal that entered Work and is waiting on this confirmation.
    #[serde(default)]
    pub source_intent_id: Option<String>,
}

/// Agent 主入口
///
/// Chat vs Work。Work (including saved presets) uses a persistent model/tool loop.
pub struct Agent {
    /// 执行引擎
    /// Shared persistence used by confirmation hand-offs across backend replicas.
    pub(crate) db: DatabaseConnection,
}
