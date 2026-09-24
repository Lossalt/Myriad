//! 顺序执行路径：一次只跑一个步骤（带智能重试），成功后处理生成器、动态步骤注入 DAG
//! 与追问判定。

use crate::services::agent::capability::CapabilityRef;
use crate::services::agent::tier_router::TierRouter;
use crate::services::agent::types::{self, *};
use serde_json::Value;

use super::Executor;
use super::executor_footer::tapp_interaction_wait_question;
use super::run_state::{PausePersist, RunState, StepFlow, record_skill_evolution};
use super::{dag, frontend_ack, retry};

impl Executor {
    /// 执行单个步骤。`is_dynamic` 为 true 时跳过生成器与追问分析。
    pub(super) async fn run_serial_step(
        &self,
        run: &mut RunState<'_>,
        step: RecipeStep,
        is_dynamic: bool,
    ) -> StepFlow {
        // Skill 编排步骤（skill: 前缀）：只是生成子步骤，不直接面向用户，跳过前端进度
        let is_skill_planning = CapabilityRef::parse(&step.capability_id).is_skill();

        // 计算前端显示用的总步骤数和当前序号
        let effective_total = run.effective_total();
        run.task_state.current_step = run.step_index;
        run.task_state.update_progress(effective_total);

        // 发送步骤开始事件（Skill 编排步骤不发送）
        if !is_skill_planning {
            let step_description = crate::services::agent::capability::get_step_description(&step);
            run.emitter
                .step_started(
                    &step.id,
                    run.display_step_counter as u32,
                    effective_total as u32,
                    &step_description,
                    crate::services::agent::response_agent::describe_step_start(&step_description),
                )
                .await;
            run.display_step_counter += 1;
        } else {
            run.hidden_skill_steps += 1;
        }
        run.emit_debug_start(&step, is_dynamic).await;

        // 执行步骤（带智能重试）——委托给统一的 retry 模块
        let pre_dynamic_count = run.context.pending_dynamic_steps.len();
        let step_display_index = run.display_step_counter.saturating_sub(1) as u32;
        let mut retry_config = retry::RetryConfig {
            max_attempts: Self::default_max_retries(&step),
            global_budget: run.global_retry_budget,
        };
        let event_ctx = retry::RetryEventContext {
            step_display_index,
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

        // 注入错误分析器建议的前置步骤
        if !outcome.prepend_steps.is_empty() {
            run.context
                .queue_dynamic_steps(outcome.prepend_steps.clone());
        }

        let duration_ms = outcome.duration_ms;

        if outcome.success {
            let output = frontend_ack::publish_and_await_snapshots(
                &run.emitter,
                &run.task_state.task_id,
                &step.id,
                &step.capability_id,
                step_display_index,
                duration_ms,
                outcome.output.clone().unwrap_or_default(),
                &mut run.context,
                !is_skill_planning,
            )
            .await;
            run.emitter
                .debug_complete(
                    &step.id,
                    &step.capability_id,
                    is_dynamic,
                    duration_ms,
                    true,
                    None,
                    None,
                )
                .await;

            if let Some((question, persist)) = self
                .on_serial_step_succeeded(
                    run,
                    &step,
                    is_dynamic,
                    &outcome,
                    output,
                    pre_dynamic_count,
                )
                .await
            {
                return StepFlow::WaitForInput(question, persist);
            }
        } else {
            // 步骤失败
            let error_msg = outcome.error.as_deref().unwrap_or("unknown error");

            if !is_skill_planning {
                run.emitter
                    .step_failed(&step.id, step_display_index, duration_ms, error_msg)
                    .await;
            }
            run.emitter
                .debug_complete(
                    &step.id,
                    &step.capability_id,
                    is_dynamic,
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

        // 记录步骤追踪
        let tier_str = if TierRouter::requires_llm(&step.capability_id) {
            format!("{:?}", outcome.last_tier)
        } else {
            String::new()
        };
        run.record_trace(
            &step,
            tier_str,
            duration_ms,
            outcome.success,
            outcome.error.clone(),
            is_dynamic,
        );

        StepFlow::Next
    }

    /// 成功后的收尾；需要暂停等待用户输入时返回问题与落库方式
    async fn on_serial_step_succeeded(
        &self,
        run: &mut RunState<'_>,
        step: &RecipeStep,
        is_dynamic: bool,
        outcome: &retry::StepRetryOutcome,
        output: Value,
        pre_dynamic_count: usize,
    ) -> Option<(types::UserQuestion, PausePersist)> {
        // 动态步骤生成器
        if !is_dynamic {
            if let Some(ref r#gen) = step.generator {
                let generated = self
                    .process_step_generator(r#gen, step, &output, &mut run.context)
                    .await;
                if !generated.is_empty() {
                    tracing::info!(
                        step_id = %step.id,
                        count = generated.len(),
                        "[Executor] Generator produced {} dynamic steps",
                        generated.len()
                    );
                    run.context.queue_dynamic_steps(generated);
                }
            }
        }

        // 更新动态步骤计数
        let post_dynamic_count = run.context.pending_dynamic_steps.len();
        if post_dynamic_count > pre_dynamic_count {
            let new_count = post_dynamic_count - pre_dynamic_count;
            run.dynamic_steps_queued += new_count;

            if new_count > 1 {
                Self::inject_dynamic_steps_into_dag(run, pre_dynamic_count, new_count);
            }
        }

        let mut result = outcome.to_step_result(&step.id);
        result.output = Some(output.clone());
        run.task_state.step_results.insert(step.id.clone(), result);

        if let Some(ref mut dag) = run.dag_scheduler {
            dag.mark_completed(&step.id);
        }

        record_skill_evolution(&step.capability_id, true, None).await;

        if let Some(question) = tapp_interaction_wait_question(&step.capability_id, &output) {
            return Some((question, PausePersist::AwaitTappWait));
        }

        // 动态分析：检查是否需要用户输入
        if !is_dynamic {
            if let Some(question) = self
                .analyze_and_generate_dynamic_steps(step, &output, &mut run.context, run.recipe)
                .await
            {
                return Some((question, PausePersist::Background));
            }
        }

        None
    }

    /// 多个新动态步骤并入 DAG（没有 DAG 时新建并切到并行模式）
    fn inject_dynamic_steps_into_dag(
        run: &mut RunState<'_>,
        pre_dynamic_count: usize,
        new_count: usize,
    ) {
        let new_steps: Vec<RecipeStep> =
            run.context.pending_dynamic_steps[pre_dynamic_count..].to_vec();
        let injected = if let Some(ref mut dag) = run.dag_scheduler {
            dag.add_steps(&new_steps).is_ok()
        } else {
            match dag::DagScheduler::new(&new_steps) {
                Ok(new_dag) => {
                    run.dag_scheduler = Some(new_dag);
                    true
                }
                Err(e) => {
                    tracing::warn!(error = %e, "[Executor] Failed to create DAG for dynamic steps");
                    false
                }
            }
        };
        if injected {
            for s in &run.context.pending_dynamic_steps[pre_dynamic_count..] {
                run.dag_injected_ids.insert(s.id.clone());
            }
            run.context.pending_dynamic_steps.drain(pre_dynamic_count..);
            if !run.use_dag {
                run.use_dag = true;
                tracing::info!(task_id = %run.task_state.task_id, count = new_count, "[Executor] Dynamic steps injected into DAG, enabling parallel mode");
            } else {
                tracing::info!(task_id = %run.task_state.task_id, count = new_count, "[Executor] Dynamic steps injected into existing DAG");
            }
        }
    }
}
