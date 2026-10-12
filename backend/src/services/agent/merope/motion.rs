//! Strict-Lite semantic motion selection for Merope.
//!
//! The model selects a bounded expression/posture baseline, semantic cues, and
//! grounded `phrases`. Anime2.5DRig driver values, lip sync, blinking, breathing
//! and secondary motion remain deterministic on the client.

use std::time::{Duration, Instant};

use myriad_merope::{
    ChatPerformanceBaseline, ChatPerformanceCue, ChatPerformancePlan,
    PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS, PERFORMANCE_INTERRUPT_MODES,
    PERFORMANCE_PHRASE_INTENTS, PERFORMANCE_POSTURES, RIG_STATE_MOTION_STYLES, RigStateSummary,
    ScoreBeat, SpeechPhrase, cue_is_playable, cue_survives_state, grounded_score,
    grounded_speech_phrases, parse_performance_plan, plan_is_empty, refine_performance_plan,
    round_motion_style,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::entities::agent_persona;

use super::MoodTransition;
use super::motion_local::local_performance_plan;
use super::store::get_persona;

mod prompt;
mod questions;
mod requests;

use prompt::*;
#[cfg(test)]
pub(in crate::services::agent) use prompt::{semantic_contract, semantic_valid};
pub use requests::*;

/// MOTION_TIMEOUT 9s; MOTION_TOTAL_TIMEOUT 10s.
/// Streaming publishes `local_directive` first; Lite is a refinement.
const MOTION_TIMEOUT: Duration = Duration::from_secs(9);
const MOTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const MOTION_SCHEMA_NAME: &str = "merope_motion";
/// A standing score's period: long enough not to read as a loop, short enough to come round.
pub(super) const STANDING_LOOP_MS: u32 = 20_000;
pub(super) const STANDING_MIN_LOOP_MS: u32 = 6_000;
pub(super) const STANDING_MAX_LOOP_MS: u32 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionPhase {
    Reaction,
    Delivery,
    Outcome,
    Proactive,
    Mood,
    /// Nobody is talking with her: how she acts what she is doing on her own.
    Presence,
}

impl MotionPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reaction => "reaction",
            Self::Delivery => "delivery",
            Self::Outcome => "outcome",
            Self::Proactive => "proactive",
            Self::Mood => "mood",
            Self::Presence => "presence",
        }
    }

    pub fn activity(self) -> &'static str {
        match self {
            Self::Reaction => "thinking",
            Self::Delivery | Self::Outcome | Self::Proactive => "talking",
            Self::Mood | Self::Presence => "idle",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MotionContext {
    pub user_id: i32,
    pub phase: MotionPhase,
    pub mood: MoodTransition,
    pub activity: String,
    pub user_text: String,
    pub response_text: Option<String>,
    /// Recently issued intentions, not proof that the body has played them.
    pub previous_phrases: Vec<SpeechPhrase>,
    pub task_success: Option<bool>,
    pub rig_state: Option<RigStateSummary>,
    /// Resolved once per Chat/Work round. Client style is only a fallback.
    pub motion_style: String,
}

#[derive(Debug, Clone, PartialEq)]
enum MotionDecision {
    Continue,
    Perform(ChatPerformancePlan),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceDirective {
    pub phase: MotionPhase,
    pub mood_revision: i64,
    pub motion_style: String,
    pub plan: ChatPerformancePlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<SpeechPhrase>,
    /// Beats on the reply's words or on time, so she can act while speaking,
    /// listening, thinking or idle alike.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub score: Vec<ScoreBeat>,
    /// Standing acting only: the score plays again each this many milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_ms: Option<u32>,
}

/// The model that directs her acting: the judgment model, quick and thinking
/// little, since acting is a small typed judgment that must land in time;
/// Lite's own model when no judgment model is set.
pub(in crate::services::agent) async fn director_analyzer(
    timeout: Duration,
) -> Option<crate::services::analyzer::AiAnalyzer> {
    match crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(timeout)).await {
        Some(judge) => Some(judge),
        None => {
            crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(timeout)).await
        }
    }
}

/// The judgment model's source when it answers decisions (TypeSafe Jev)
/// rather than writing a plan.
pub(in crate::services::agent) async fn decisions_source() -> Option<crate::config::ResolvedAiConfig>
{
    crate::services::ai::learn_judge_model_kind().await;
    crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_decisions_ai_config()
}

/// What the judgment model makes of a moment by answering the questions, as
/// the director's plan: on the decisions endpoint when `source` is given,
/// otherwise `analyzer` answers as structured output.
#[cfg(test)]
pub(in crate::services::agent) async fn answered_plan(
    context: &MotionContext,
    persona: Option<&str>,
    source: Option<&crate::config::ResolvedAiConfig>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> anyhow::Result<String> {
    let state = context.rig_state.as_ref();
    match source {
        Some(source) => questions::direct(context, state, persona, source, MOTION_TIMEOUT).await,
        None => questions::direct_by_chat(context, state, persona, analyzer).await,
    }
}

/// Runs exactly one Lite-tier call, falling back to the deterministic plan for
/// callers (such as proactive speech) that have no live floor publisher.
pub async fn direct_motion(context: MotionContext) -> Option<PerformanceDirective> {
    direct_motion_inner(context, true).await
}

/// Refines a floor that has already been published. A timeout, invalid answer
/// or `continue` produces nothing so the same local beat is never replayed.
pub async fn refine_motion(context: MotionContext) -> Option<PerformanceDirective> {
    direct_motion_inner(context, false).await
}

async fn direct_motion_inner(
    context: MotionContext,
    fallback_to_local: bool,
) -> Option<PerformanceDirective> {
    let started = Instant::now();
    let phase = context.phase.as_str();
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        tracing::debug!(phase, "[MeropeMotion] Face hidden; ambient motion only");
        return None;
    }
    // The judgment model judges her acting by answering typed questions: a
    // decisions model (TypeSafe Jev) on its own endpoint, a chat model as
    // structured output. Her standing acting too: the answers become a score
    // looped on time. The chat director writes the plan only when answering
    // fails.
    let decided = {
        let persona_row = match crate::services::process_db::database() {
            Ok(db) => get_persona(&db).await.ok().flatten(),
            Err(_) => None,
        };
        let persona = persona_row
            .as_ref()
            .map(|row| row.personality.trim().to_owned());
        let persona = persona.as_deref().filter(|p| !p.is_empty());
        let call = async {
            if let Some(source) = decisions_source().await {
                questions::direct(
                    &context,
                    rig_state.as_ref(),
                    persona,
                    &source,
                    MOTION_TIMEOUT,
                )
                .await
            } else if let Some(analyzer) = director_analyzer(MOTION_TIMEOUT).await {
                questions::direct_by_chat(&context, rig_state.as_ref(), persona, &analyzer).await
            } else {
                Err(anyhow::anyhow!("no judgment model"))
            }
        };
        Some(
            tokio::time::timeout(
                MOTION_TOTAL_TIMEOUT,
                crate::services::ai_cost_ledger::with_site_ai_ledger(
                    context.user_id,
                    "merope",
                    &format!("motion_{phase}"),
                    call,
                ),
            )
            .await,
        )
    };
    let chat_director = async {
        let Some(analyzer) = director_analyzer(MOTION_TIMEOUT).await else {
            tracing::debug!(
                phase,
                "[MeropeMotion] Lite unavailable; requested cues may still play"
            );
            return None;
        };
        let persona_row = match crate::services::process_db::database() {
            Ok(db) => get_persona(&db).await.ok().flatten(),
            Err(_) => None,
        };
        let input = motion_input(&context, persona_row.as_ref()).to_string();

        // Offer only what this face can actually play. Constrained decoding
        // then cannot spend the round's 0–2 cues on something the filter below
        // would delete.
        let offered = offered_cue_intents(rig_state.as_ref());
        let schema = motion_schema_for_state(&offered, rig_state.as_ref());
        let system_prompt = motion_system_prompt(&offered);
        let call = analyzer.analyze_json(&system_prompt, &input, MOTION_SCHEMA_NAME, Some(&schema));
        Some(
            tokio::time::timeout(
                MOTION_TOTAL_TIMEOUT,
                crate::services::ai_cost_ledger::with_site_ai_ledger(
                    context.user_id,
                    "merope",
                    &format!("motion_{phase}"),
                    call,
                ),
            )
            .await,
        )
    };
    let result = or_chat_director(decided, phase, chat_director).await;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let mut phrases = Vec::new();
    let mut score = Vec::new();
    let mut loop_ms = None;
    let lite_plan = match result {
        None => None,
        Some(Ok(Ok(raw))) => match parse_motion_decision(&raw) {
            Some(MotionDecision::Continue) => {
                tracing::debug!(
                    phase,
                    elapsed_ms,
                    "[MeropeMotion] Lite continued current acting"
                );
                // Deliberate continuation is not a failed model call. In
                // particular, direct/proactive callers must not replay a local
                // fallback baseline when the director asked to leave it alone.
                return None;
            }
            Some(MotionDecision::Perform(parsed)) => {
                if let Ok(value) = serde_json::from_str::<Value>(strip_motion_json(&raw)) {
                    phrases = grounded_speech_phrases(
                        &value["phrases"],
                        context.response_text.as_deref(),
                    );
                    score = grounded_score(
                        &value["score"],
                        context.response_text.as_deref(),
                        rig_state
                            .as_ref()
                            .map(|state| state.capabilities.as_slice()),
                    );
                    if context.phase == MotionPhase::Presence && !score.is_empty() {
                        loop_ms = Some(
                            value["loopMs"]
                                .as_u64()
                                .unwrap_or(STANDING_LOOP_MS as u64)
                                .clamp(STANDING_MIN_LOOP_MS as u64, STANDING_MAX_LOOP_MS as u64)
                                as u32,
                        );
                    }
                }
                Some(parsed)
            }
            None => {
                tracing::warn!(
                    phase,
                    elapsed_ms,
                    "[MeropeMotion] Invalid Lite plan dropped"
                );
                None
            }
        },
        Some(Ok(Err(error))) => {
            tracing::warn!(phase, elapsed_ms, error = %error, "[MeropeMotion] Lite call dropped");
            None
        }
        Some(Err(_)) => {
            tracing::warn!(
                phase,
                elapsed_ms,
                "[MeropeMotion] Lite total timeout; plan dropped"
            );
            None
        }
    };
    let refined_by_lite = lite_plan.is_some();
    let mut plan = if let Some(plan) = lite_plan {
        plan
    } else if fallback_to_local {
        local_performance_plan(
            context.phase,
            &context.mood,
            context.task_success,
            context.response_text.as_deref(),
            &context.motion_style,
            Some(&context.user_text),
        )
    } else {
        return None;
    };
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
    // A named ask is not a command; overlay only after this turn's reply.
    plan = apply_user_requested_cue(
        plan,
        context.phase,
        &context.user_text,
        context.response_text.as_deref(),
        rig_state.as_ref(),
    );
    if plan_is_empty(&plan) && phrases.is_empty() && score.is_empty() {
        tracing::debug!(
            phase,
            elapsed_ms,
            "[MeropeMotion] Lite plan had no capable cues"
        );
        return None;
    }
    tracing::info!(
        phase,
        elapsed_ms,
        source = if refined_by_lite { "lite" } else { "local" },
        requested = play_along_requested_cue(
            context.phase,
            &context.user_text,
            context.response_text.as_deref(),
        )
        .is_some(),
        "[MeropeMotion] plan ready"
    );
    Some(PerformanceDirective {
        phase: context.phase,
        mood_revision: context.mood.revision,
        motion_style: context.motion_style,
        plan,
        phrases,
        score,
        loop_ms,
    })
}

type DirectorAnswer = Option<Result<anyhow::Result<String>, tokio::time::error::Elapsed>>;

/// The plan from the answered questions; the chat director's when none were
/// asked (standing acting), or when answering failed rather than ran out of
/// time.
async fn or_chat_director(
    decided: DirectorAnswer,
    phase: &str,
    chat_director: impl std::future::Future<Output = DirectorAnswer>,
) -> DirectorAnswer {
    match decided {
        Some(Ok(Err(error))) => {
            tracing::warn!(phase, error = %error, "[MeropeMotion] Answering the acting questions failed; asking the chat director");
            chat_director.await
        }
        None => chat_director.await,
        answered => answered,
    }
}

/// The deterministic floor as a directive, with no network call and no await.
///
/// Chat plays this the moment the round starts so the character reacts before
/// it speaks. Non-streaming attaches it on the body; streaming Chat leaves
/// `performance: None` and Lite replaces via PlaybackDirection / `PerformancePlan`.
pub fn local_directive(context: &MotionContext) -> Option<PerformanceDirective> {
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        return None;
    }
    let mut plan = local_performance_plan(
        context.phase,
        &context.mood,
        context.task_success,
        context.response_text.as_deref(),
        &context.motion_style,
        Some(&context.user_text),
    );
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
    plan = apply_user_requested_cue(
        plan,
        context.phase,
        &context.user_text,
        context.response_text.as_deref(),
        rig_state.as_ref(),
    );
    if plan_is_empty(&plan) {
        return None;
    }
    Some(PerformanceDirective {
        phase: context.phase,
        mood_revision: context.mood.revision,
        motion_style: context.motion_style.clone(),
        plan,
        phrases: Vec::new(),
        score: Vec::new(),
        loop_ms: None,
    })
}

/// One persona read per Chat/Work round. Client style is only used when the
/// site persona cannot be loaded.
pub async fn resolve_round_motion_style(
    client: Option<&RigStateSummary>,
    mood: i32,
    arousal: i32,
) -> String {
    let client_style = client.map(|summary| summary.motion_style.as_str());
    if let Ok(db) = crate::services::process_db::database() {
        if let Ok(Some(persona)) = get_persona(&db).await {
            return round_motion_style(
                client_style,
                persona.persona_json.as_ref(),
                true,
                mood,
                arousal,
            );
        }
    }
    round_motion_style(client_style, None, false, mood, arousal)
}

fn apply_round_motion_style(
    summary: Option<RigStateSummary>,
    motion_style: &str,
) -> Option<RigStateSummary> {
    let mut summary = summary?;
    summary.motion_style = if RIG_STATE_MOTION_STYLES.contains(&motion_style) {
        motion_style.to_string()
    } else {
        summary.motion_style
    };
    Some(summary)
}

fn face_is_hidden(state: Option<&RigStateSummary>) -> bool {
    state.is_some_and(|summary| !summary.page_visible || !summary.face_visible)
}

fn parse_motion_decision(raw: &str) -> Option<MotionDecision> {
    let stripped = strip_motion_json(raw);
    let value: serde_json::Value = serde_json::from_str(stripped).ok()?;
    let object = value.as_object()?;
    match object.get("continue") {
        Some(flag) if !flag.is_boolean() => return None,
        Some(flag) if flag.as_bool() == Some(true) => {
            if motion_payload_present(object) {
                return None;
            }
            return Some(MotionDecision::Continue);
        }
        _ => {}
    }
    if let Some(plan) = parse_performance_plan(stripped) {
        return Some(MotionDecision::Perform(plan));
    }
    // A grounded phrase refinement does not need to replace the standing face.
    // Do not turn an invalid baseline/cue payload into a phrase-only success.
    let empty_plan = object.get("baseline").is_none_or(Value::is_null)
        && object
            .get("cues")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty));
    let valid_score = !grounded_score(
        object.get("score").unwrap_or(&Value::Null),
        // Words are checked against the reply later; here only the beat's shape.
        Some(&scored_words(object)),
        None,
    )
    .is_empty();
    let valid_phrase = object
        .get("phrases")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().take(6).any(|item| {
                !grounded_speech_phrases(
                    &serde_json::json!([item]),
                    item.get("text").and_then(Value::as_str),
                )
                .is_empty()
            })
        });
    (empty_plan && (valid_phrase || valid_score))
        .then(|| MotionDecision::Perform(ChatPerformancePlan::default()))
}

/// The words a score's beats name, so its shape can be checked without the reply.
fn scored_words(object: &serde_json::Map<String, Value>) -> String {
    object
        .get("score")
        .and_then(Value::as_array)
        .map(|beats| {
            beats
                .iter()
                .filter_map(|beat| beat.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn motion_payload_present(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    let has_baseline = object.get("baseline").is_some_and(|value| !value.is_null());
    let has_cues = object
        .get("cues")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|cues| !cues.is_empty());
    has_baseline
        || has_cues
        || object
            .get("score")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        || object
            .get("phrases")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
}

fn strip_motion_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim()
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
pub(super) mod live_probe {
    use super::*;

    pub fn contract(
        persona: &agent_persona::Model,
        rig: &RigStateSummary,
    ) -> (String, Value, Value) {
        let offered = offered_cue_intents(Some(rig));
        (
            motion_system_prompt(&offered),
            motion_schema(&offered),
            motion_persona_payload(Some(persona), "even"),
        )
    }

    pub fn valid(raw: &str, rig: &RigStateSummary) -> bool {
        let Ok(unfiltered) = serde_json::from_str::<ChatPerformancePlan>(raw) else {
            return false;
        };
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(raw) else {
            return false;
        };
        // A smoke test must not pass merely because production's safe parser
        // removed an invalid cue or clamped an out-of-range value.
        plan == unfiltered
            && plan.cues.len() <= 2
            && plan.baseline.is_some()
            && plan
                .cues
                .iter()
                .all(|cue| cue_survives_state(rig, &cue.intent))
    }

    #[test]
    fn smoke_rejects_cues_discarded_by_the_production_sanitizer() {
        let rig = myriad_merope::sanitize_rig_state(&serde_json::json!({})).unwrap();
        let valid = serde_json::json!({"baseline":{"expression":"steady","posture":"neutral","motionEnergy":1.0,"attention":0.5},"cues":[]});
        assert!(self::valid(&valid.to_string(), &rig));
        let mut invalid = valid;
        invalid["cues"] = serde_json::json!([{"intent":"not_a_cue","atMs":0,"intensity":1.0,"tempo":1.0,"fadeInMs":100,"fadeOutMs":200,"interrupt":"blend"}]);
        assert!(!self::valid(&invalid.to_string(), &rig));
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;

#[cfg(test)]
mod decisions_fallback_tests {
    use super::*;

    fn chat() -> impl std::future::Future<Output = DirectorAnswer> {
        async { Some(Ok(Ok("chat".to_owned()))) }
    }

    #[tokio::test]
    async fn a_failed_decisions_call_is_answered_by_the_chat_director() {
        let failed: DirectorAnswer = Some(Ok(Err(anyhow::anyhow!("decisions call failed: 502"))));
        let answer = or_chat_director(failed, "delivery", chat()).await;
        assert_eq!(answer.unwrap().unwrap().unwrap(), "chat");
    }

    #[tokio::test]
    async fn a_decisions_answer_stands_and_a_spent_budget_is_not_spent_again() {
        let decided: DirectorAnswer = Some(Ok(Ok("jev".to_owned())));
        let answer = or_chat_director(decided, "delivery", chat()).await;
        assert_eq!(answer.unwrap().unwrap().unwrap(), "jev");
        let elapsed = tokio::time::timeout(Duration::ZERO, std::future::pending::<()>())
            .await
            .unwrap_err();
        let timed_out: DirectorAnswer = Some(Err(elapsed));
        assert!(
            or_chat_director(timed_out, "delivery", chat())
                .await
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn without_a_decisions_model_the_chat_director_answers_as_before() {
        let answer = or_chat_director(None, "delivery", chat()).await;
        assert_eq!(answer.unwrap().unwrap().unwrap(), "chat");
    }
}
