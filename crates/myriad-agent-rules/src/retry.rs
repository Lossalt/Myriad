//! Recipe step failure strategy and retry configuration types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 失败处理策略
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FailureStrategy {
    /// 终止整个方案
    Abort,
    /// 跳过并继续
    Skip,
    /// 使用默认值继续
    UseDefault(Value),
    /// 回退到备用能力
    Fallback(String),
}

/// 重试配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_attempts: u32,
    /// 重试间隔（毫秒）
    pub delay_ms: u64,
    /// 指数退避
    pub exponential_backoff: bool,
}
