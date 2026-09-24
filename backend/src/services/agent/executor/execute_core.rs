// Executor core: run recipe / process entry

use crate::config::ModelTier;
use crate::services::agent::tier_router::{self, TierRouter};
use crate::services::agent::types::{self, *};
use crate::services::ai::create_ai_analyzer_for_tier;
use crate::services::analyzer::AiAnalyzer;
use sea_orm::DatabaseConnection;
use serde_json::Value;

use super::Executor;
use super::run_state::{MAX_TOTAL_STEPS, RunState, StepFlow};
use super::{TASK_STORE, is_cancelled, task_store};

impl Executor {
    pub(crate) fn should_block_unconfirmed_dynamic_step(user_id: i32, risk: RiskLevel) -> bool {
        // Aligned with system_sensitive_gate: Medium and above blocked, including heartbeat.
        crate::services::agent::executor_resolve_pure::should_block_unconfirmed_dynamic_step(
            user_id, risk,
        )
    }

    /// 创建新的执行引擎
    pub async fn new(db: DatabaseConnection) -> Self {
        let pro_analyzer = create_ai_analyzer_for_tier(ModelTier::Pro).await;
        let standard_analyzer = create_ai_analyzer_for_tier(ModelTier::Standard).await;
        Self {
            db,
            pro_analyzer,
            standard_analyzer,
        }
    }

    /// Pro→Pro else Standard；Standard→Standard else Pro；Lite→Standard else Pro。
    pub(crate) fn get_analyzer_for_tier(&self, tier: ModelTier) -> Option<&AiAnalyzer> {
        match tier {
            ModelTier::Pro => self
                .pro_analyzer
                .as_ref()
                .or(self.standard_analyzer.as_ref()),
            ModelTier::Standard => self
                .standard_analyzer
                .as_ref()
                .or(self.pro_analyzer.as_ref()),
            // Lite：没有独立 analyzer，用 Standard，再缺则 Pro。
            ModelTier::Lite => self
                .standard_analyzer
                .as_ref()
                .or(self.pro_analyzer.as_ref()),
        }
    }

    /// 带熔断器的 tier 解析
    ///
    /// 熔断器返回 None 时仍走 `resolve_with_override`（warn）。
    pub(crate) fn resolve_tier_with_breaker(
        capability_id: &str,
        explicit_tier: Option<ModelTier>,
    ) -> ModelTier {
        tier_router::resolve_with_circuit_breaker(capability_id, explicit_tier).unwrap_or_else(
            || {
                // 熔断器返回 None 时仍尝试 resolve_with_override。
                tracing::warn!(
                    capability_id = capability_id,
                    "[Executor] Both tiers circuit-broken, falling back to default resolve"
                );
                TierRouter::resolve_with_override(capability_id, explicit_tier)
            },
        )
    }

    /// 记录步骤执行结果到熔断器
    pub(crate) fn record_step_to_breaker(tier: ModelTier, success: bool) {
        let breaker = tier_router::get_circuit_breaker(tier);
        if success {
            breaker.record_success();
        } else {
            breaker.record_failure();
        }
    }

    /// 执行方案（`progress_tx = None`）
    pub async fn execute(&self, recipe: &Recipe, user_id: i32) -> Result<TaskState, String> {
        self.execute_with_progress(recipe, user_id, None).await
    }

    /// 执行方案（带实时进度回调）
    pub async fn execute_with_progress(
        &self,
        recipe: &Recipe,
        user_id: i32,
        progress_tx: Option<tokio::sync::mpsc::Sender<types::AgentProgressEvent>>,
    ) -> Result<TaskState, String> {
        // Full-site AI usage: attribute every nested AiAnalyzer call (including admin).
        let attr = crate::services::ai_cost_ledger::AiLedgerAttribution {
            subject_id: user_id,
            owner_id: user_id,
            source: "agent".into(),
            operation: "agent".into(),
            tapp_id: "__agent__".into(),
            task_id: recipe.id.clone(),
        };
        crate::services::ai_cost_ledger::with_ai_ledger_attribution(attr, async {
            self.execute_with_progress_inner(recipe, user_id, progress_tx)
                .await
        })
        .await
    }

    async fn execute_with_progress_inner(
        &self,
        recipe: &Recipe,
        user_id: i32,
        progress_tx: Option<tokio::sync::mpsc::Sender<types::AgentProgressEvent>>,
    ) -> Result<TaskState, String> {
        // 创建任务状态
        let mut task_state = TaskState::new(recipe);
        let _cancellation = task_store::CancellationGuard::new(&task_state.task_id);
        task_state.status = TaskStatus::Running;
        task_state.lane_id = recipe.lane_key.clone();

        let context = Self::prepare_execution_context(recipe, &task_state.task_id, user_id).await;

        // 存储任务
        {
            let mut store = TASK_STORE.write().await;
            store.store(user_id, task_state.clone());
        }

        tracing::info!(
            task_id = %task_state.task_id,
            recipe_id = %recipe.id,
            steps = recipe.steps.len(),
            "[Executor] Starting recipe execution"
        );

        let mut run = RunState::new(recipe, user_id, progress_tx, task_state, context);

        while run.has_work() {
            // 检查任务是否被取消
            if is_cancelled(&run.task_state.task_id).await {
                return run.exit_cancelled().await;
            }

            // 全局步骤上限检查
            if run.total_executed_steps >= MAX_TOTAL_STEPS {
                tracing::warn!(
                    task_id = %run.task_state.task_id,
                    executed = run.total_executed_steps,
                    "[Executor] Global step limit reached ({}), stopping execution",
                    MAX_TOTAL_STEPS
                );
                // 清空待执行的动态步骤
                run.context.pending_dynamic_steps.clear();
                break;
            }
            run.total_executed_steps += 1;

            let flow = match Self::next_step(&mut run) {
                NextStep::Stop => break,
                NextStep::Parallel(ready) => self.run_parallel_wave(&mut run, ready).await,
                NextStep::Serial { step, is_dynamic } => {
                    self.run_serial_step(&mut run, *step, is_dynamic).await
                }
            };
            if let StepFlow::WaitForInput(question, persist) = flow {
                return run.pause_for_input(question, persist).await;
            }
        }

        run.finish().await
    }

    /// 选出下一步：动态步骤优先；DAG 模式下就绪多于一个则整波并行；否则按原始顺序
    fn next_step(run: &mut RunState<'_>) -> NextStep {
        if let Some(dynamic_step) = run.context.pop_dynamic_step() {
            tracing::info!(
                step_id = %dynamic_step.id,
                "[Executor] Executing dynamic step"
            );
            return NextStep::Serial {
                step: Box::new(dynamic_step),
                is_dynamic: true,
            };
        }
        if run.use_dag {
            // DAG 模式：获取所有就绪步骤
            let mut ready = run
                .dag_scheduler
                .as_ref()
                .map(|d| d.get_ready_steps())
                .unwrap_or_default();
            if ready.len() > 1 {
                return NextStep::Parallel(ready);
            }
            // 单个就绪步骤，走顺序路径
            let Some(next) = ready.pop() else {
                return NextStep::Stop;
            };
            run.advance_past(&next.id);
            let is_injected = run.dag_injected_ids.contains(&next.id);
            return NextStep::Serial {
                step: Box::new(next),
                is_dynamic: is_injected,
            };
        }
        if run.step_index < run.all_steps.len() {
            let step = run.all_steps[run.step_index].clone();
            run.step_index += 1;
            return NextStep::Serial {
                step: Box::new(step),
                is_dynamic: false,
            };
        }
        NextStep::Stop
    }

    /// 由 recipe 构建执行上下文：对话历史、路由/窗口等变量、角色身份、记忆召回、页面上下文
    async fn prepare_execution_context(
        recipe: &Recipe,
        task_id: &str,
        user_id: i32,
    ) -> ExecutionContext {
        // 创建执行上下文（包含对话历史）
        let mut context = ExecutionContext::from_request_full(
            &recipe.original_request,
            &recipe.name,
            recipe.page_context.clone(),
            recipe.conversation_context.clone(),
        );
        context.autonomy_permission_cap = recipe.autonomy_permission_cap.clone();
        context
            .variables
            .insert("_task_id".to_string(), Value::String(task_id.to_string()));
        if let Some(route) = recipe.metadata.get("current_route").cloned() {
            context
                .variables
                .insert("_current_route".to_string(), route);
        }
        if let Some(music) = recipe.metadata.get("music_status").cloned() {
            context.variables.insert("_music_status".to_string(), music);
        }
        if let Some(windows) = recipe.metadata.get("window_state").cloned() {
            context
                .variables
                .insert("_window_state".to_string(), windows);
        }

        // 记录对话上下文信息
        if let Some(ref history) = context.conversation_context {
            tracing::info!(
                task_id = %task_id,
                history_len = history.len(),
                "[Executor] Conversation context loaded with {} messages",
                history.len()
            );
        }

        // 注入角色身份上下文（从 Orchestrator 分析结果）
        if let Some(role_ctx_val) = recipe.metadata.get("role_contexts") {
            if let Some(obj) = role_ctx_val.as_object() {
                for (role_key, ctx_val) in obj {
                    if let Some(ctx_str) = ctx_val.as_str() {
                        context
                            .role_contexts
                            .insert(role_key.clone(), ctx_str.to_string());
                    }
                }
                tracing::info!(
                    task_id = %task_id,
                    roles = context.role_contexts.len(),
                    "[Executor] Role identity contexts loaded"
                );
            }
        }

        // 召回长期/中期记忆到 context.memory_context。
        if let Some(mem) = crate::services::agent::memory::get_memory() {
            use crate::services::agent::memory::{MemoryTier, RecallQuery};
            let memories = mem
                .recall_with_params(RecallQuery {
                    query: recipe.original_request.clone(),
                    limit: 4,
                    tier_filter: Some(vec![MemoryTier::LongTerm, MemoryTier::MediumTerm]),
                    user_id: Some(user_id),
                    ..Default::default()
                })
                .await;
            if !memories.is_empty() {
                let lines: Vec<String> = memories
                    .iter()
                    .map(|m| {
                        let content: String = m.content.chars().take(200).collect();
                        if m.content.chars().count() > 200 {
                            format!("- {}...", content)
                        } else {
                            format!("- {}", content)
                        }
                    })
                    .collect();
                context.memory_context = Some(lines.join("\n"));
                tracing::info!(
                    task_id = %task_id,
                    memories = memories.len(),
                    "[Executor] Memory context loaded for AI steps"
                );
            }
        }

        // 如果有 page_context，存入 step_outputs
        if let Some(page_ctx) = context.page_context.clone() {
            let page_title = page_ctx
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            context.add_output("__page_context__", page_ctx);
            tracing::info!(
                task_id = %task_id,
                page_title = %page_title,
                "[Executor] Page context stored as __page_context__"
            );
        }

        context
    }
}

/// 主循环每一轮选出的工作
enum NextStep {
    /// 没有可执行的步骤，结束主循环
    Stop,
    /// DAG 中同时就绪的多个步骤，整波并行执行
    Parallel(Vec<RecipeStep>),
    /// 顺序执行单个步骤；`is_dynamic` 表示动态步骤或 DAG 注入步骤
    Serial {
        step: Box<RecipeStep>,
        is_dynamic: bool,
    },
}
