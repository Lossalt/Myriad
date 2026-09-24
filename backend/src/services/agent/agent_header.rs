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

/// Agent 主入口
///
/// Chat vs Work。Work (including saved presets) uses a persistent model/tool loop.
pub struct Agent {
    pub(crate) db: DatabaseConnection,
}
