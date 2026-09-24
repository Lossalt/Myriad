//! Fixed workflows are resumable frames, with each effect crossing work_tool.
use super::*;
use crate::models::entities::agent_task_presets as presets;
use crate::services::agent::capability::CapabilityRef;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct RecipeRun {
    pub call: ToolCall,
    pub preset_id: i32,
    pub steps: Vec<RecipeStep>,
    pub cursor: usize,
    pub active_call: Option<String>,
    pub outputs: HashMap<String, Value>,
    pub results: Vec<Value>,
    /// Started by the preset API rather than a model `run_recipe` call.
    #[serde(default)]
    pub direct: bool,
}

/// Hand the aggregate back to whoever started the frame.
fn finish_frame(state: &mut Checkpoint, frame: RecipeRun, output: &Value) {
    if frame.direct {
        state.direct_recipe_result(frame.call, output);
    } else {
        state.tool_result(frame.call, output);
    }
}

pub(super) async fn list(db: &sea_orm::DatabaseConnection, user: i32) -> Result<Value, String> {
    let rows = presets::Entity::find()
        .filter(presets::Column::UserId.eq(user))
        .filter(presets::Column::ParsedSteps.is_not_null())
        .order_by_desc(presets::Column::Id)
        .limit(20)
        .all(db)
        .await
        .map_err(|_| "Cannot load saved recipes")?;
    Ok(json!({"recipes":rows.into_iter().map(|row| {
        let recipe = row.parsed_steps.and_then(|value| serde_json::from_value::<Recipe>(value).ok());
        let supported = recipe.clone().map(compile).transpose();
        json!({"id":row.id,"name":row.title.or(row.intent_summary),"steps":recipe.map(|r|r.steps.len()),"supported":matches!(supported,Ok(Some(_)))})
    }).collect::<Vec<_>>()}))
}

fn compile(mut recipe: Recipe) -> Result<Vec<RecipeStep>, String> {
    for step in &mut recipe.steps {
        if step.generator.is_some()
            || step.retry.is_some()
            || matches!(step.on_failure, FailureStrategy::Fallback(_))
        {
            return Err("This tool supports fixed recipes without generators, automatic retries or fallback branches".into());
        }
        for (key, value) in &step.params {
            if key.ends_with("From") {
                if step.params.contains_key(key.trim_end_matches("From")) {
                    return Err("Recipe input cannot contain both a literal and a reference for the same parameter".into());
                }
                let reference = value.as_str().ok_or("Recipe reference must be a string")?;
                let dependency = reference.split('.').next().unwrap_or_default().to_owned();
                if !step.depends_on.contains(&dependency) {
                    step.depends_on.push(dependency);
                }
            }
        }
    }
    Agent::validate_saved_recipe(&recipe)?;
    let mut done = HashSet::new();
    let mut sorted = Vec::new();
    while !recipe.steps.is_empty() {
        let index = recipe
            .steps
            .iter()
            .position(|step| step.depends_on.iter().all(|id| done.contains(id)))
            .ok_or("Recipe dependency cycle")?;
        let step = recipe.steps.remove(index);
        done.insert(step.id.clone());
        sorted.push(step);
    }
    Ok(sorted)
}

pub(super) async fn start(
    db: &sea_orm::DatabaseConnection,
    state: &mut Checkpoint,
    call: ToolCall,
    id: i32,
) -> Result<(), String> {
    if state.recipe_run.is_some() {
        return Err("A saved recipe is already running".into());
    }
    let row = presets::Entity::find_by_id(id)
        .filter(presets::Column::UserId.eq(state.user_id))
        .one(db)
        .await
        .map_err(|_| "Cannot load saved recipe")?
        .ok_or("Saved recipe not found")?;
    let recipe: Recipe =
        serde_json::from_value(row.parsed_steps.ok_or("Saved recipe has no steps")?)
            .map_err(|_| "Invalid saved recipe")?;
    let steps = compile(recipe)?;
    if state.calls + state.pending.len() + steps.len() >= MAX_CALLS {
        return Err("Recipe exceeds remaining tool-call budget".into());
    }
    for step in &steps {
        if capability::get_capability_by_id(&step.capability_id)
            .await
            .is_none()
            || CapabilityRef::parse(&step.capability_id).is_skill()
        {
            return Err(format!(
                "Unsupported recipe capability: {}",
                step.capability_id
            ));
        }
    }
    state.pending.pop_front();
    state.calls += 1;
    *state
        .call_counts
        .entry(operation_key(&call.name, &json!({"preset_id":id})))
        .or_default() += 1;
    state.recipe_run = Some(RecipeRun {
        call,
        preset_id: id,
        steps,
        cursor: 0,
        active_call: None,
        outputs: HashMap::new(),
        results: vec![],
        direct: false,
    });
    Ok(())
}

pub(super) fn abort(state: &mut Checkpoint, reason: &str) {
    if let Some(mut frame) = state.recipe_run.take() {
        // A completed child may not yet have been folded by advance when user
        // steering or recovery arrives. Never hide its committed/unknown effect.
        if let Some(id) = frame.active_call.as_ref() {
            if !frame.results.iter().any(|result| result["call_id"] == *id) {
                if let Some(result) = state.task.step_results.get(id) {
                    frame.results.push(json!({"step":frame.steps[frame.cursor].id,"call_id":id,"success":result.success,"output":result.output,"error":result.error}));
                } else {
                    frame.results.push(json!({"step":frame.steps[frame.cursor].id,"call_id":id,"success":false,"error":"Step interrupted before its outcome was recorded; inspect the target before repeating"}));
                }
            }
        }
        let output = json!({"recipeId":frame.preset_id,"steps":frame.results,"error":reason});
        finish_frame(state, frame, &output);
    }
}

/// Returns true when the frame changed and should be checkpointed before work.
pub(super) fn advance(
    state: &mut Checkpoint,
    executor: &executor::Executor,
) -> Result<bool, String> {
    let Some(frame) = state.recipe_run.as_mut() else {
        return Ok(false);
    };
    if let Some(id) = &frame.active_call {
        if state.pending.iter().any(|pending| &pending.call.id == id) {
            return Ok(false);
        }
        let result = state
            .task
            .step_results
            .get(id)
            .ok_or("Missing recipe step result")?;
        let step = &frame.steps[frame.cursor];
        frame.results.push(json!({"step":step.id,"call_id":id,"success":result.success,"output":result.output,"error":result.error}));
        if result.success {
            frame.outputs.insert(
                step.id.clone(),
                result.output.clone().unwrap_or(Value::Null),
            );
        } else {
            match &step.on_failure {
                FailureStrategy::Skip => {}
                FailureStrategy::UseDefault(value) => {
                    frame.outputs.insert(step.id.clone(), value.clone());
                }
                _ => {
                    abort(state, "Saved recipe stopped after a failed step");
                    return Ok(true);
                }
            }
        }
        frame.cursor += 1;
        frame.active_call = None;
    }
    if frame.cursor == frame.steps.len() {
        let frame = state.recipe_run.take().unwrap();
        let output = json!({"recipeId":frame.preset_id,"steps":frame.results,"success":true});
        finish_frame(state, frame, &output);
        return Ok(true);
    }
    let step = &frame.steps[frame.cursor];
    let (params, unresolved) = executor.resolve_params(&step.params, &frame.outputs);
    if !unresolved.is_empty() {
        abort(state, "Saved recipe has unresolved input references");
        return Ok(true);
    }
    let id = format!("recipe_{}", uuid::Uuid::new_v4());
    frame.active_call = Some(id.clone());
    state.pending.push_front(PendingCall {
        call: ToolCall {
            id,
            name: tools::tool_name(&step.capability_id),
            arguments: json!(params).to_string(),
        },
        capability_id: Some(step.capability_id.clone()),
        approval: None,
    });
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::work_loop::tests::checkpoint;

    pub(super) fn step(id: &str, capability: &str, params: Value) -> RecipeStep {
        RecipeStep {
            id: id.into(),
            order: 0,
            capability_id: capability.into(),
            action: id.into(),
            params: serde_json::from_value(params).unwrap(),
            depends_on: vec![],
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    #[test]
    fn references_define_order_and_cycles_fail_before_effects() {
        let mut recipe = checkpoint().task.recipe.unwrap();
        recipe.steps = vec![
            step(
                "save",
                "note.create",
                json!({"contentFrom":"read.data[0].text"}),
            ),
            step("read", "data.transform", json!({})),
        ];
        assert_eq!(compile(recipe.clone()).unwrap()[0].id, "read");
        recipe.steps[1]
            .params
            .insert("inputFrom".into(), json!("save"));
        assert!(compile(recipe).is_err());
    }

    /// A preset started by the API answers no assistant tool call: its
    /// aggregate joins the user turn so no provider sees an orphan tool result.
    #[test]
    fn direct_recipe_results_join_the_user_turn() {
        let mut state = checkpoint();
        let before = state.history.len();
        state.recipe_run = Some(RecipeRun {
            call: ToolCall {
                id: "preset_1".into(),
                name: "run_recipe".into(),
                arguments: "{}".into(),
            },
            preset_id: 1,
            steps: vec![step("one", "time.info", json!({}))],
            cursor: 0,
            active_call: None,
            outputs: HashMap::new(),
            results: vec![],
            direct: true,
        });
        abort(&mut state, "Interrupted");
        assert!(state.task.step_results.contains_key("preset_1"));
        assert!(
            !state
                .history
                .iter()
                .any(|message| matches!(message, ToolMessage::Tool { .. }))
        );
        let Some(ToolMessage::User { content }) = state.history.last() else {
            panic!("aggregate must land in a user message");
        };
        assert!(content.contains("<untrusted_recipe_result>"));
        assert!(content.contains("Interrupted"));
        assert!(state.history.len() <= before + 1);
    }

    #[test]
    fn internal_results_do_not_create_orphan_provider_messages() {
        let mut state = checkpoint();
        let parent = ToolCall {
            id: "parent".into(),
            name: "run_recipe".into(),
            arguments: "{}".into(),
        };
        state.recipe_run = Some(RecipeRun {
            call: parent,
            preset_id: 1,
            steps: vec![step("one", "time.info", json!({}))],
            cursor: 0,
            active_call: Some("child".into()),
            outputs: HashMap::new(),
            results: vec![],
            direct: false,
        });
        state.tool_result(
            ToolCall {
                id: "child".into(),
                name: "time".into(),
                arguments: "{}".into(),
            },
            &json!({"value":1}),
        );
        assert_eq!(state.history.len(), 1);
        assert!(state.task.step_results["child"].success);
        abort(&mut state, "Interrupted");
        let aggregate = state.task.step_results["parent"].output.as_ref().unwrap();
        assert_eq!(aggregate["steps"][0]["call_id"], "child");
        assert_eq!(aggregate["steps"][0]["output"]["value"], 1);
        assert!(
            matches!(state.history.last(),Some(ToolMessage::Tool {call,..}) if call.id=="parent")
        );
    }
}
