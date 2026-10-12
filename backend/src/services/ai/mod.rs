//! AI 服务通用模块
//!
//! 提供统一的 AI 分析器创建和管理，消除各模块重复代码

mod analyzer_factory;
mod model_kind;

pub use analyzer_factory::*;
pub use model_kind::learn_judge_model_kind;
