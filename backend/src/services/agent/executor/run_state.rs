//! 一次 Recipe 执行的运行态，以及主循环的三个出口：用户取消、暂停等待输入、正常收尾。

use crate::services::agent::types::{self, *};
use std::collections::{HashMap, HashSet};

use super::Executor;
use super::{TASK_STORE, clear_cancellation, is_cancelled, persist_task_async};
use super::{dag, events, task_store};

/// 全局已执行步骤上限（防止动态步骤导致无限执行）
pub(super) const MAX_TOTAL_STEPS: usize = 15;

/// 全局重试预算（跨所有步骤最多重试 5 次）
const GLOBAL_RETRY_BUDGET: u32 = 5;

/// 单步 / 并行波次跑完后，主循环下一步做什么
pub(super) enum StepFlow {
    /// 继续主循环
    Next,
    /// 暂停并等待用户回答
    WaitForInput(types::UserQuestion, PausePersist),
}

/// 暂停时任务状态的落库方式
pub(super) enum PausePersist {
    /// 后台持久化（`persist_task_async`）
    Background,
    /// 同步落库，失败则整个执行返回错误（Tapp 交互等待）
    AwaitTappWait,
}

/// 主循环在一次执行中持有的全部可变状态
pub(super) struct RunState<'r> {
    pub(super) recipe: &'r Recipe,
    pub(super) user_id: i32,
    pub(super) progress_tx: Option<tokio::sync::mpsc::Sender<types::AgentProgressEvent>>,
    /// SSE 事件发送器
    pub(super) emitter: events::StepEventEmitter,
    pub(super) task_state: TaskState,
    pub(super) context: ExecutionContext,

    // 执行追踪
    execution_start: std::time::Instant,
    trace_id: String,
    step_traces: Vec<types::StepTrace>,
    tier_usage: HashMap<String, u32>,

    pub(super) global_retry_budget: u32,
    /// 全局已执行步骤计数器
    pub(super) total_executed_steps: usize,

    pub(super) all_steps: Vec<RecipeStep>,
    pub(super) total_steps: usize,
    pub(super) step_index: usize,

    /// 前端步骤显示用：已入队的动态步骤总数（不随 pop 减少）
    pub(super) dynamic_steps_queued: usize,
    /// 前端步骤显示用：已发送 StepStarted 的次数（用作 display index）
    pub(super) display_step_counter: usize,
    /// 被隐藏的 Skill 编排步骤数量（用于修正 effective_total）
    pub(super) hidden_skill_steps: usize,

    pub(super) dag_scheduler: Option<dag::DagScheduler>,
    pub(super) use_dag: bool,
    /// 注入 DAG 的动态步骤 ID（相对原始 recipe 步骤）
    pub(super) dag_injected_ids: HashSet<String>,
}

impl<'r> RunState<'r> {
    pub(super) fn new(
        recipe: &'r Recipe,
        user_id: i32,
        progress_tx: Option<tokio::sync::mpsc::Sender<types::AgentProgressEvent>>,
        task_state: TaskState,
        context: ExecutionContext,
    ) -> Self {
        // 初始化执行追踪
        let execution_start = std::time::Instant::now();
        let trace_id = format!("trace_{}", task_state.task_id);
        let emitter = events::StepEventEmitter::new(progress_tx.clone());

        let all_steps: Vec<RecipeStep> = recipe.steps.clone();
        let total_steps = all_steps.len();

        // 构建 DAG 调度器（检测是否有并行依赖）
        let dag_scheduler = dag::DagScheduler::new(&all_steps).ok();
        let use_dag = dag_scheduler.as_ref().is_some_and(|d| d.is_parallel_mode())
            && !all_steps
                .iter()
                .any(|step| step.capability_id == "tapp.interact");
        if use_dag {
            tracing::info!(
                task_id = %task_state.task_id,
                "[Executor] Parallel DAG mode detected, using DAG scheduler"
            );
        }

        Self {
            recipe,
            user_id,
            progress_tx,
            emitter,
            task_state,
            context,
            execution_start,
            trace_id,
            step_traces: Vec::new(),
            tier_usage: HashMap::new(),
            global_retry_budget: GLOBAL_RETRY_BUDGET,
            total_executed_steps: 0,
            all_steps,
            total_steps,
            step_index: 0,
            dynamic_steps_queued: 0,
            display_step_counter: 0,
            hidden_skill_steps: 0,
            dag_scheduler,
            use_dag,
            dag_injected_ids: HashSet::new(),
        }
    }

    /// 主循环条件：原始步骤未走完、仍有动态步骤、或 DAG 仍有剩余
    pub(super) fn has_work(&self) -> bool {
        self.step_index < self.all_steps.len()
            || self.context.has_pending_steps()
            || self
                .dag_scheduler
                .as_ref()
                .is_some_and(|d| d.has_remaining())
    }

    /// 前端显示用的总步骤数
    pub(super) fn effective_total(&self) -> usize {
        (self.total_steps + self.dynamic_steps_queued).saturating_sub(self.hidden_skill_steps)
    }

    /// 把 `step_index` 推进到原始步骤列表中该步骤之后
    /// Parallel steps finish out of order; the cursor only moves forward.
    pub(super) fn advance_past(&mut self, step_id: &str) {
        if let Some(idx) = self.all_steps.iter().position(|s| s.id == step_id) {
            self.step_index = self.step_index.max(idx + 1);
        }
    }

    pub(super) async fn emit_debug_start(&self, step: &RecipeStep, is_dynamic: bool) {
        self.emitter
            .debug_start(
                &step.id,
                &step.capability_id,
                if step.action.is_empty() {
                    None
                } else {
                    Some(step.action.clone())
                },
                if self.context.original_request.is_empty() {
                    None
                } else {
                    Some(self.context.original_request.clone())
                },
                Executor::build_debug_params(&step.params),
                is_dynamic,
            )
            .await;
    }

    /// 记录一条步骤追踪（tier 为空表示非 LLM 步骤，不计入 tier_usage）
    pub(super) fn record_trace(
        &mut self,
        step: &RecipeStep,
        tier_used: String,
        duration_ms: u64,
        success: bool,
        error: Option<String>,
        is_dynamic: bool,
    ) {
        if !tier_used.is_empty() {
            *self.tier_usage.entry(tier_used.clone()).or_insert(0) += 1;
        }
        self.step_traces.push(types::StepTrace {
            step_id: step.id.clone(),
            capability_id: step.capability_id.clone(),
            tier_used,
            duration_ms,
            success,
            error,
            action: step.action.clone(),
            params: serde_json::to_value(&step.params).ok(),
            output_preview: None,
            is_dynamic,
        });
    }

    /// 用户取消：写入 Cancelled、发送取消事件并落库
    pub(super) async fn exit_cancelled(mut self) -> Result<TaskState, String> {
        tracing::info!(
            task_id = %self.task_state.task_id,
            "[Executor] Task cancelled by user"
        );
        self.task_state.status = TaskStatus::Cancelled;
        self.task_state.completed_at = Some(chrono::Utc::now());
        self.task_state.error =
            Some(crate::services::agent::response_agent::task_cancelled_by_user());

        // 清除取消标记
        clear_cancellation(&self.task_state.task_id).await;

        // 发送取消事件
        if let Some(ref tx) = self.progress_tx {
            let _ = tx
                .send(AgentProgressEvent::Error {
                    task_id: Some(self.task_state.task_id.clone()),
                    message: crate::services::agent::response_agent::task_cancelled(),
                    code: "CANCELLED".to_string(),
                })
                .await;
        }

        // 更新存储
        sync_task_store(&self.task_state).await;
        persist_task_async(self.user_id, self.task_state.clone());

        Ok(self.task_state)
    }

    /// 暂停等待用户输入：保存 recipe 与执行上下文以便 resume
    pub(super) async fn pause_for_input(
        self,
        question: types::UserQuestion,
        persist: PausePersist,
    ) -> Result<TaskState, String> {
        let Self {
            recipe,
            user_id,
            emitter,
            mut task_state,
            mut context,
            global_retry_budget,
            ..
        } = self;

        // 保存任务状态为 WaitingForInput
        task_state.status = TaskStatus::WaitingForInput;
        task_state.set_pending_question(question.clone());
        task_state.recipe = Some(recipe.clone());
        context.retry_budget_remaining = global_retry_budget;
        task_state.execution_context = Some(context);

        // A Tapp interaction wait must be durable before anyone is told the
        // task is waiting: its answer arrives through the database.
        if let PausePersist::AwaitTappWait = persist {
            if let Err(error) = task_store::save_task_to_db(user_id, &task_state).await {
                tracing::error!(%error, "persist Tapp interaction wait state failed");
                let message = "Failed to persist Tapp interaction wait state".to_string();
                task_state.status = TaskStatus::Failed;
                task_state.error = Some(message.clone());
                task_state.completed_at = Some(chrono::Utc::now());
                sync_task_store(&task_state).await;
                return Err(message);
            }
        }

        emitter
            .waiting_for_input(&task_state.task_id, &question)
            .await;
        sync_task_store(&task_state).await;
        if let PausePersist::Background = persist {
            persist_task_async(user_id, task_state.clone());
        }

        Ok(task_state)
    }

    /// 主循环正常结束：附加执行追踪、按步骤结果定终态并落库
    pub(super) async fn finish(self) -> Result<TaskState, String> {
        let Self {
            user_id,
            mut task_state,
            execution_start,
            trace_id,
            step_traces,
            tier_usage,
            ..
        } = self;

        // A cancellation can arrive while the final long-running step is in
        // flight. Recheck before committing a terminal success so a remote
        // replica's cancelled DB state cannot be overwritten as completed.
        let cancelled = is_cancelled(&task_state.task_id).await;
        if cancelled {
            clear_cancellation(&task_state.task_id).await;
        }

        // 附加执行追踪
        task_state.execution_trace = Some(types::ExecutionTrace {
            trace_id,
            steps: step_traces,
            total_duration_ms: execution_start.elapsed().as_millis() as u64,
            tier_usage,
            planner_decision: None,
        });

        // 根据步骤结果决定最终状态
        let total_steps = task_state.step_results.len();
        let failed_steps = task_state
            .step_results
            .values()
            .filter(|r| !r.success)
            .count();
        if cancelled {
            task_state.status = TaskStatus::Cancelled;
            task_state.error =
                Some(crate::services::agent::response_agent::task_cancelled_by_user());
        } else if failed_steps > 0 && failed_steps == total_steps {
            task_state.status = TaskStatus::Failed;
            let errors: Vec<String> = task_state
                .step_results
                .values()
                .filter_map(|r| r.error.clone())
                .collect();
            task_state.error = Some(errors.join("; "));
        } else {
            task_state.status = TaskStatus::Completed;
        }
        task_state.completed_at = Some(chrono::Utc::now());
        task_state.progress = 100;

        sync_task_store(&task_state).await;
        persist_task_async(user_id, task_state.clone());

        Ok(task_state)
    }
}

/// 用当前快照覆盖内存 TASK_STORE 中已存在的同名任务
async fn sync_task_store(task_state: &TaskState) {
    let mut store = TASK_STORE.write().await;
    if let Some(task) = store.get_mut(&task_state.task_id) {
        *task = task_state.clone();
    }
}

/// 技能进化统计（未启用时为空操作）
pub(super) async fn record_skill_evolution(
    capability_id: &str,
    success: bool,
    error: Option<&str>,
) {
    if let Some(evo) = crate::services::agent::skill_evolution::get_skill_evolution() {
        evo.on_execution_complete(capability_id, success, error)
            .await;
    }
}
