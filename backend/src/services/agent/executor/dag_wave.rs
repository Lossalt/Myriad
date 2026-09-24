//! 流式 DAG 并行波次：一次就绪多于一个步骤时并行启动，任何步骤完成即补发新就绪步骤，
//! 波次结束后处理暂挂的用户问题，再对可重试失败步骤走串行重试。
//!
//! 使用 FuturesUnordered 而非 join_all：慢步骤不阻塞快步骤的后续依赖。

use crate::services::agent::error_analyzer_pure::StepError;
use crate::services::agent::tier_router::TierRouter;
use crate::services::agent::types::{self, *};
use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use super::Executor;
use super::handlers::HandlerContext;
use super::is_cancelled;
use super::run_state::{MAX_TOTAL_STEPS, PausePersist, RunState, StepFlow, record_skill_evolution};
use super::{error_analyzer, frontend_ack, retry};

/// 并行步骤在独立 context 快照上跑完后回传的结果
struct DagStepDone {
    step: RecipeStep,
    result: Result<Value, StepError>,
    duration_ms: u64,
    /// 快照上新增的动态步骤
    dynamic_steps: Vec<RecipeStep>,
    /// 快照上的全部 variables（合并时只补主 context 缺失的键）
    variables: HashMap<String, Value>,
    /// 快照上新增的决策记录
    decisions: Vec<types::ExecutionDecision>,
}

type StepFuture<'s> = Pin<Box<dyn Future<Output = DagStepDone> + Send + 's>>;

/// 一个并行波次内的局部状态
struct DagWave<'s> {
    effective_total: usize,
    spawned: HashSet<String>,
    in_flight: FuturesUnordered<StepFuture<'s>>,
    /// (step, first_error, first_duration_ms)
    failed_for_retry: Vec<(RecipeStep, String, u64)>,
    /// 暂挂的用户问题队列：检测到后停止启动新步骤，等 in-flight 自然完成
    pending_questions: Vec<types::UserQuestion>,
}

impl Executor {
    /// 执行一个并行波次（`ready.len() > 1`）
    pub(super) async fn run_parallel_wave(
        &self,
        run: &mut RunState<'_>,
        ready: Vec<RecipeStep>,
    ) -> StepFlow {
        let mut wave = DagWave {
            effective_total: run.effective_total(),
            spawned: HashSet::new(),
            in_flight: FuturesUnordered::new(),
            failed_for_retry: Vec::new(),
            pending_questions: Vec::new(),
        };

        // 启动所有初始就绪步骤
        for step in &ready {
            wave.spawned.insert(step.id.clone());
            self.launch_dag_step(run, &mut wave, step).await;
        }
        // 外层循环已计 1，修正计数
        run.total_executed_steps = run.total_executed_steps.saturating_sub(1);

        tracing::info!(
            task_id = %run.task_state.task_id,
            initial = ready.len(),
            "[Executor] Streaming DAG: launched {} initial steps",
            ready.len()
        );

        // Cancelled mid-wave: skip pausing and retries; the driver loop's
        // cancellation check records the cancelled state.
        let dag_cancelled = self.stream_dag_wave(run, &mut wave).await;
        if !dag_cancelled && !wave.pending_questions.is_empty() {
            let question = wave.pending_questions.remove(0);
            // 将剩余问题存入 context，resume 后继续提问
            if !wave.pending_questions.is_empty() {
                tracing::info!(
                    deferred = wave.pending_questions.len(),
                    "[Executor] DAG: {} additional questions stored for later",
                    wave.pending_questions.len()
                );
                run.context.pending_questions.extend(wave.pending_questions);
            }
            return StepFlow::WaitForInput(question, PausePersist::Background);
        }

        // 流式DAG后的串行重试（复用 retry.rs 统一逻辑）
        // 如果已取消，跳过所有重试
        if !dag_cancelled {
            self.retry_failed_dag_steps(run, wave.failed_for_retry)
                .await;
        }

        run.task_state.update_progress(wave.effective_total);
        // 继续下一波并行步骤
        StepFlow::Next
    }

    /// 发送开始事件并把步骤放进 in-flight（调用方负责登记 `spawned`）
    async fn launch_dag_step<'s>(
        &'s self,
        run: &mut RunState<'_>,
        wave: &mut DagWave<'s>,
        step: &RecipeStep,
    ) {
        run.total_executed_steps += 1;

        let desc = crate::services::agent::capability::get_step_description(step);
        run.emitter
            .step_started(
                &step.id,
                run.display_step_counter as u32,
                wave.effective_total as u32,
                &desc,
                crate::services::agent::response_agent::describe_parallel_step_start(&desc),
            )
            .await;
        run.emit_debug_start(step, run.dag_injected_ids.contains(&step.id))
            .await;
        run.display_step_counter += 1;

        let step_tier = Self::resolve_tier_with_breaker(&step.capability_id, step.model_tier);
        let step_analyzer = self.get_analyzer_for_tier(step_tier);
        let ctx_snapshot = run.context.clone();
        let step_clone = step.clone();
        let executor_task_id = run.task_state.task_id.clone();
        let user_id = run.user_id;
        wave.in_flight.push(Box::pin(async move {
            let start = std::time::Instant::now();
            let mut ctx = ctx_snapshot;
            let handler_ctx = HandlerContext {
                db: &self.db,
                ai_analyzer: step_analyzer,
                user_id,
                task_id: Some(executor_task_id),
                step_id: Some(step_clone.id.clone()),
                execution_context: Some(ctx.clone()),
                autonomy_permission_cap: ctx.autonomy_permission_cap.clone(),
            };
            let pre_dyn = ctx.pending_dynamic_steps.len();
            let pre_decisions = ctx.decision_history.len();
            let result = self.execute_step(&step_clone, &mut ctx, &handler_ctx).await;
            let dynamic_steps = ctx.pending_dynamic_steps[pre_dyn..].to_vec();
            // 收集并行步骤新增的 variables 和 decisions，回传给主 context
            let variables = ctx.variables.clone();
            let decisions = ctx.decision_history[pre_decisions..].to_vec();
            DagStepDone {
                step: step_clone,
                result,
                duration_ms: start.elapsed().as_millis() as u64,
                dynamic_steps,
                variables,
                decisions,
            }
        }));
    }

    /// 流式处理：每完成一个步骤，立即检查并启动新就绪步骤。返回是否被用户取消。
    async fn stream_dag_wave<'s>(&'s self, run: &mut RunState<'_>, wave: &mut DagWave<'s>) -> bool {
        while let Some(done) = wave.in_flight.next().await {
            // 取消检查：在流式循环中也能及时响应取消
            if is_cancelled(&run.task_state.task_id).await {
                tracing::info!(
                    task_id = %run.task_state.task_id,
                    "[Executor] DAG streaming cancelled by user"
                );
                return true;
            }

            let par_tier =
                TierRouter::resolve_with_override(&done.step.capability_id, done.step.model_tier);
            let par_tier_str = if TierRouter::requires_llm(&done.step.capability_id) {
                format!("{:?}", par_tier)
            } else {
                String::new()
            };
            run.advance_past(&done.step.id);

            let DagStepDone {
                step,
                result,
                duration_ms,
                dynamic_steps,
                variables,
                decisions,
            } = done;
            match result {
                Ok(output) => {
                    Self::record_step_to_breaker(par_tier, true);
                    // 合并并行步骤产生的 variables 和 decisions 到主 context
                    for (k, v) in variables {
                        run.context.variables.entry(k).or_insert(v);
                    }
                    run.context.decision_history.extend(decisions);

                    self.on_dag_step_succeeded(
                        run,
                        wave,
                        step,
                        output,
                        duration_ms,
                        dynamic_steps,
                        par_tier_str,
                    )
                    .await;
                }
                Err(error) => {
                    Self::record_step_to_breaker(par_tier, false);
                    Self::on_dag_step_failed(run, wave, step, error, duration_ms, par_tier_str)
                        .await;
                }
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    async fn on_dag_step_succeeded<'s>(
        &'s self,
        run: &mut RunState<'_>,
        wave: &mut DagWave<'s>,
        step: RecipeStep,
        output: Value,
        duration_ms: u64,
        returned_dynamic_steps: Vec<RecipeStep>,
        par_tier_str: String,
    ) {
        let is_injected = run.dag_injected_ids.contains(&step.id);

        let output = frontend_ack::publish_and_await_snapshots(
            &run.emitter,
            &run.task_state.task_id,
            &step.id,
            &step.capability_id,
            0,
            duration_ms,
            output,
            &mut run.context,
            true,
        )
        .await;
        let output_preview = {
            let s = serde_json::to_string(&output).unwrap_or_default();
            if s.len() > 1000 {
                format!("{}...", &s[..1000])
            } else {
                s
            }
        };
        run.emitter
            .debug_complete(
                &step.id,
                &step.capability_id,
                is_injected,
                duration_ms,
                true,
                Some(output_preview),
                None,
            )
            .await;

        run.task_state.step_results.insert(
            step.id.clone(),
            StepResult {
                step_id: step.id.clone(),
                success: true,
                output: Some(output),
                error: None,
                duration_ms,
                retry_count: 0,
            },
        );

        if let Some(ref mut dag) = run.dag_scheduler {
            dag.mark_completed(&step.id);
        }

        record_skill_evolution(&step.capability_id, true, None).await;

        // 合并从 execute_step 返回的动态步骤（技能子步骤等）
        // pre_dynamic_count 在 generator 之前取值，确保 generator 和 skill 子步骤都能被 DAG 注入
        let pre_dynamic_count = run.context.pending_dynamic_steps.len();

        // process_step_generator，除非已是 DAG 注入步骤。
        if !is_injected {
            if let Some(ref r#gen) = step.generator {
                if let Some(ref output_val) = run
                    .task_state
                    .step_results
                    .get(&step.id)
                    .and_then(|r| r.output.clone())
                {
                    let generated = self
                        .process_step_generator(r#gen, &step, output_val, &mut run.context)
                        .await;
                    if !generated.is_empty() {
                        tracing::info!(
                            step_id = %step.id,
                            count = generated.len(),
                            "[Executor] DAG generator produced {} dynamic steps",
                            generated.len()
                        );
                        run.context.queue_dynamic_steps(generated);
                    }
                }
            }
        }

        if !returned_dynamic_steps.is_empty() {
            tracing::info!(
                step_id = %step.id,
                count = returned_dynamic_steps.len(),
                "[Executor] DAG step returned {} dynamic steps from executor",
                returned_dynamic_steps.len()
            );
            run.context.queue_dynamic_steps(returned_dynamic_steps);
        }

        // 动态步骤并行化：注入 DAG 调度器
        let post_dynamic_count = run.context.pending_dynamic_steps.len();
        if post_dynamic_count > pre_dynamic_count {
            let new_count = post_dynamic_count - pre_dynamic_count;
            // Same count as the serial path, so the displayed total agrees.
            run.dynamic_steps_queued += new_count;
            if new_count > 1 {
                let new_steps: Vec<RecipeStep> =
                    run.context.pending_dynamic_steps[pre_dynamic_count..].to_vec();
                let injected = if let Some(ref mut dag) = run.dag_scheduler {
                    dag.add_steps(&new_steps).is_ok()
                } else {
                    false
                };
                if injected {
                    for s in &run.context.pending_dynamic_steps[pre_dynamic_count..] {
                        run.dag_injected_ids.insert(s.id.clone());
                    }
                    run.context.pending_dynamic_steps.drain(pre_dynamic_count..);
                    tracing::info!(
                        task_id = %run.task_state.task_id,
                        count = new_count,
                        "[Executor] DAG parallel: dynamic steps injected into DAG"
                    );
                }
            }
        }

        run.record_trace(&step, par_tier_str, duration_ms, true, None, is_injected);

        // 动态分析：检查是否需要用户输入
        // DAG注入的动态步骤不触发分析，防止链式膨胀
        if !is_injected {
            if let Some(ref output_val) = run
                .task_state
                .step_results
                .get(&step.id)
                .and_then(|r| r.output.clone())
            {
                if let Some(question) = self
                    .analyze_and_generate_dynamic_steps(
                        &step,
                        output_val,
                        &mut run.context,
                        run.recipe,
                    )
                    .await
                {
                    tracing::info!(
                        step_id = %step.id,
                        question_id = %question.question_id,
                        queued = wave.pending_questions.len(),
                        "[Executor] DAG parallel: step output requires user input, queuing question"
                    );
                    wave.pending_questions.push(question);
                    // 不再启动新步骤，让 in-flight 自然完成
                }
            }
        }

        // 没有暂挂问题时立刻补发新就绪步骤
        if wave.pending_questions.is_empty() {
            let newly_ready = run
                .dag_scheduler
                .as_ref()
                .map(|dag| dag.get_ready_steps())
                .unwrap_or_default();
            for new_step in newly_ready {
                if wave.spawned.contains(&new_step.id) {
                    continue;
                }
                // The run-wide step cap holds inside a wave too; what is left
                // stays in the DAG and the driver loop stops at the cap.
                if run.total_executed_steps >= MAX_TOTAL_STEPS {
                    break;
                }
                wave.spawned.insert(new_step.id.clone());
                self.launch_dag_step(run, wave, &new_step).await;

                tracing::info!(
                    step_id = %new_step.id,
                    "[Executor] Streaming DAG: immediately spawned newly-ready step"
                );
            }
        }
    }

    /// 可重试的失败步骤进入波次后重试队列；否则立即记为失败
    async fn on_dag_step_failed(
        run: &mut RunState<'_>,
        wave: &mut DagWave<'_>,
        step: RecipeStep,
        error: StepError,
        duration_ms: u64,
        par_tier_str: String,
    ) {
        let is_injected = run.dag_injected_ids.contains(&step.id);

        let analysis = error_analyzer::ErrorAnalyzer::analyze(
            &error.message,
            &step.capability_id,
            &step.params,
        );
        let max_retries = crate::services::agent::retry_pure::default_max_retries(&step);
        let retryable = crate::services::agent::error_analyzer_pure::may_retry_step(
            analysis.retryable,
            crate::services::agent::capability::is_effectful(&step.capability_id).await,
            &error,
        );
        let e = error.message;

        if retryable && run.global_retry_budget > 0 && max_retries > 1 {
            tracing::info!(
                step_id = %step.id,
                category = ?analysis.category,
                "[Executor] Streaming DAG step failed, queuing for retry: {}",
                analysis.description
            );
            wave.failed_for_retry.push((step, e, duration_ms));
            return;
        }

        tracing::error!(
            step_id = %step.id,
            error = %e,
            "[Executor] Streaming DAG step failed (not retryable)"
        );

        run.emitter.step_failed(&step.id, 0, duration_ms, &e).await;
        run.emitter
            .debug_complete(
                &step.id,
                &step.capability_id,
                is_injected,
                duration_ms,
                false,
                None,
                Some(e.clone()),
            )
            .await;

        record_skill_evolution(&step.capability_id, false, Some(&e)).await;

        run.task_state.step_results.insert(
            step.id.clone(),
            StepResult {
                step_id: step.id.clone(),
                success: false,
                output: None,
                error: Some(e.clone()),
                duration_ms,
                retry_count: 0,
            },
        );

        if let Some(ref mut dag) = run.dag_scheduler {
            dag.mark_failed(&step.id, &step.on_failure);
        }

        run.record_trace(
            &step,
            par_tier_str,
            duration_ms,
            false,
            Some(e),
            is_injected,
        );
    }

    /// 波次后的串行重试：DAG 已失败一次，这里从头重新执行+重试
    async fn retry_failed_dag_steps(
        &self,
        run: &mut RunState<'_>,
        failed_for_retry: Vec<(RecipeStep, String, u64)>,
    ) {
        for (step, _first_error, _first_duration) in failed_for_retry {
            let is_injected = run.dag_injected_ids.contains(&step.id);

            let max_retries = Self::default_max_retries(&step);
            let mut retry_config = retry::RetryConfig {
                max_attempts: max_retries,
                global_budget: run.global_retry_budget,
            };
            let event_ctx = retry::RetryEventContext {
                step_display_index: 0,
                progress_tx: run.progress_tx.clone(),
            };

            let outcome = self
                .execute_step_with_retry(
                    &step,
                    &mut run.context,
                    run.user_id,
                    &mut retry_config,
                    &event_ctx,
                )
                .await;
            run.global_retry_budget = retry_config.global_budget;

            let duration_ms = outcome.duration_ms;
            let tier_str = if TierRouter::requires_llm(&step.capability_id) {
                format!("{:?}", outcome.last_tier)
            } else {
                String::new()
            };

            // 注入错误分析器建议的前置步骤
            if !outcome.prepend_steps.is_empty() {
                run.context
                    .queue_dynamic_steps(outcome.prepend_steps.clone());
            }

            if outcome.success {
                let merged = frontend_ack::publish_and_await_snapshots(
                    &run.emitter,
                    &run.task_state.task_id,
                    &step.id,
                    &step.capability_id,
                    0,
                    duration_ms,
                    outcome.output.clone().unwrap_or(json!(null)),
                    &mut run.context,
                    true,
                )
                .await;
                let mut result = outcome.to_step_result(&step.id);
                result.output = Some(merged);
                run.task_state.step_results.insert(step.id.clone(), result);

                if let Some(ref mut dag) = run.dag_scheduler {
                    dag.mark_completed(&step.id);
                }

                record_skill_evolution(&step.capability_id, true, None).await;
            } else {
                let error_msg = outcome.error.as_deref().unwrap_or("unknown error");

                run.emitter
                    .step_failed(&step.id, 0, duration_ms, error_msg)
                    .await;
                run.emitter
                    .debug_complete(
                        &step.id,
                        &step.capability_id,
                        is_injected,
                        duration_ms,
                        false,
                        None,
                        Some(error_msg.to_string()),
                    )
                    .await;

                record_skill_evolution(&step.capability_id, false, Some(error_msg)).await;

                run.task_state
                    .step_results
                    .insert(step.id.clone(), outcome.to_step_result(&step.id));

                if let Some(ref mut dag) = run.dag_scheduler {
                    dag.mark_failed(&step.id, &step.on_failure);
                }
            }

            run.record_trace(
                &step,
                tier_str,
                duration_ms,
                outcome.success,
                outcome.error,
                is_injected,
            );
        }
    }
}
