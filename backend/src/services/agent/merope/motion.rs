//! Her acting, directed by one director: the judgment model answering typed
//! questions about the moment (`questions`). Its answers become a bounded
//! expression/posture baseline, cues, grounded `phrases` and a `score`; the
//! floor (`motion_local`) shows her state until it lands or when it has
//! nothing. Anime2.5DRig driver values, lip sync, blinking, breathing and
//! secondary motion remain deterministic on the client.

use std::time::{Duration, Instant};

use myriad_merope::{
    ChatPerformancePlan, RIG_STATE_MOTION_STYLES, RigStateSummary, ScoreBeat, SpeechPhrase,
    cue_is_playable, grounded_score, grounded_speech_phrases, parse_performance_plan,
    plan_is_empty, refine_performance_plan, round_motion_style,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::MoodTransition;
use super::motion_local::local_performance_plan;
use super::store::get_persona;

mod control;
mod offered;
mod presence;
mod questions;
mod way;

#[cfg(test)]
pub(in crate::services::agent) use offered::semantic_valid;
#[cfg(test)]
use offered::*;
#[cfg(test)]
pub(in crate::services::agent) use questions::semantic_contract;

/// MOTION_TIMEOUT 9s; MOTION_TOTAL_TIMEOUT 10s.
/// Streaming publishes `local_directive` first; the director refines it.
const MOTION_TIMEOUT: Duration = Duration::from_secs(9);
const MOTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
/// Standing acting waits for no reply and is kept for the whole activity, so
/// a chat model may take its time over every control (5–12 s measured).
const STANDING_TIMEOUT: Duration = Duration::from_secs(28);
const STANDING_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
/// A standing score's period (control::STANDING_LOOP_MS): long enough not
/// to read as a loop, short enough to come round.
pub(super) const STANDING_LOOP_MS: u32 = control::STANDING_LOOP_MS as u32;

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
    let budget = if context.phase == MotionPhase::Presence {
        STANDING_TIMEOUT
    } else {
        MOTION_TIMEOUT
    };
    match source {
        Some(source) => questions::direct(context, state, persona, source, budget).await,
        None => questions::direct_by_chat(context, state, persona, analyzer, budget).await,
    }
}

/// Asks the director once, falling back to the floor for callers (such as
/// proactive speech) that have no live floor publisher.
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
    // Her only director: the judgment model, answering typed questions about
    // the moment (a decisions model on its own endpoint, a chat model as
    // structured output); her standing acting too, as a score looped on time.
    // When it has no answer, the floor already playing stands.
    let persona_row = match crate::services::process_db::database() {
        Ok(db) => get_persona(&db).await.ok().flatten(),
        Err(_) => None,
    };
    let persona = persona_row
        .as_ref()
        .map(|row| row.personality.trim().to_owned());
    let persona = persona.as_deref().filter(|p| !p.is_empty());
    let (budget, total) = if context.phase == MotionPhase::Presence {
        (STANDING_TIMEOUT, STANDING_TOTAL_TIMEOUT)
    } else {
        (MOTION_TIMEOUT, MOTION_TOTAL_TIMEOUT)
    };
    let call = async {
        if let Some(source) = decisions_source().await {
            questions::direct(&context, rig_state.as_ref(), persona, &source, budget).await
        } else if let Some(analyzer) = director_analyzer(budget).await {
            questions::direct_by_chat(&context, rig_state.as_ref(), persona, &analyzer, budget)
                .await
        } else {
            Err(anyhow::anyhow!("no judgment model"))
        }
    };
    let result = tokio::time::timeout(
        total,
        crate::services::ai_cost_ledger::with_site_ai_ledger(
            context.user_id,
            "merope",
            &format!("motion_{phase}"),
            call,
        ),
    )
    .await;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let mut phrases = Vec::new();
    let mut score = Vec::new();
    let mut loop_ms = None;
    let mut asked_for = None;
    let directed = match result {
        Ok(Ok(raw)) => match parse_motion_decision(&raw) {
            Some(MotionDecision::Continue) => {
                tracing::debug!(phase, elapsed_ms, "[MeropeMotion] Director continued");
                // Deliberate continuation is not a failed call: direct and
                // proactive callers must not replay a floor over it.
                return None;
            }
            Some(MotionDecision::Perform(parsed)) => {
                if let Ok(value) = serde_json::from_str::<Value>(strip_motion_json(&raw)) {
                    asked_for = value["askedFor"].as_str().map(str::to_owned);
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
                        loop_ms = Some(STANDING_LOOP_MS);
                    }
                }
                Some(parsed)
            }
            None => {
                tracing::warn!(phase, elapsed_ms, "[MeropeMotion] Invalid plan dropped");
                None
            }
        },
        Ok(Err(error)) => {
            tracing::warn!(phase, elapsed_ms, error = %error, "[MeropeMotion] Director had no answer");
            None
        }
        Err(_) => {
            tracing::warn!(phase, elapsed_ms, "[MeropeMotion] Director timed out");
            None
        }
    };
    let directed_here = directed.is_some();
    let mut plan = if let Some(plan) = directed {
        plan
    } else if fallback_to_local {
        local_performance_plan(
            context.phase,
            &context.mood,
            context.task_success,
            &context.motion_style,
        )
    } else {
        return None;
    };
    let asked =
        asked_for.and_then(|intent| plan.cues.iter().find(|cue| cue.intent == intent).cloned());
    if let Some(state) = rig_state.as_ref() {
        plan = keep_agreed_face(refine_performance_plan(plan, state), asked, state);
    }
    if plan_is_empty(&plan) && phrases.is_empty() && score.is_empty() {
        tracing::debug!(phase, elapsed_ms, "[MeropeMotion] Plan had no capable cues");
        return None;
    }
    tracing::info!(
        phase,
        elapsed_ms,
        source = if directed_here { "director" } else { "floor" },
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

/// A face she agreed to make is made even over her words: it survives the
/// state filter whenever the face can make it.
fn keep_agreed_face(
    mut plan: ChatPerformancePlan,
    agreed: Option<myriad_merope::ChatPerformanceCue>,
    state: &RigStateSummary,
) -> ChatPerformancePlan {
    if let Some(cue) = agreed.filter(|cue| {
        cue_is_playable(&state.capabilities, &cue.intent)
            && !plan.cues.iter().any(|kept| kept.intent == cue.intent)
    }) {
        plan.cues.insert(0, cue);
        plan.cues.truncate(3);
    }
    plan
}

/// Her standing acting when the director cannot be reached (offline): shown
/// the `way` what she is doing shows by its kind (`presence::WAYS`), with no
/// network call.
pub fn standing_floor(context: &MotionContext, way: &str) -> Option<PerformanceDirective> {
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        return None;
    }
    let value = presence::standing_floor(way, context, rig_state.as_ref());
    let Some(MotionDecision::Perform(mut plan)) = parse_motion_decision(&value.to_string()) else {
        return None;
    };
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
    let score = grounded_score(
        &value["score"],
        None,
        rig_state
            .as_ref()
            .map(|state| state.capabilities.as_slice()),
    );
    Some(PerformanceDirective {
        phase: MotionPhase::Presence,
        mood_revision: context.mood.revision,
        motion_style: context.motion_style.clone(),
        plan,
        phrases: Vec::new(),
        score,
        loop_ms: Some(STANDING_LOOP_MS),
    })
}

/// The deterministic floor as a directive, with no network call and no await.
///
/// Chat plays this the moment the round starts so the character reacts before
/// it speaks. Non-streaming attaches it on the body; streaming Chat leaves
/// `performance: None` and the director replaces it via PlaybackDirection /
/// `PerformancePlan`.
pub fn local_directive(context: &MotionContext) -> Option<PerformanceDirective> {
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        return None;
    }
    let mut plan = local_performance_plan(
        context.phase,
        &context.mood,
        context.task_success,
        &context.motion_style,
    );
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
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

/// A candidate judgment model tried on the director's questions (live acceptance).
#[cfg(test)]
pub(super) mod live_probe {
    use super::*;
    use crate::models::entities::agent_persona;

    fn moment(rig: &RigStateSummary) -> MotionContext {
        MotionContext {
            user_id: 0,
            phase: MotionPhase::Delivery,
            mood: MoodTransition {
                before: 65.0,
                after: 65.0,
                arousal_before: 48.0,
                arousal_after: 48.0,
                band_before: "calm".into(),
                band_after: "calm".into(),
                delta: 0.0,
                cause: "user_utterance".into(),
                revision: 1,
            },
            activity: "talking".into(),
            user_text: String::new(),
            response_text: None,
            previous_phrases: Vec::new(),
            task_success: None,
            rig_state: Some(rig.clone()),
            motion_style: "even".into(),
        }
    }

    /// The system prompt, schema and persona a candidate answers with.
    pub fn contract(
        persona: &agent_persona::Model,
        rig: &RigStateSummary,
    ) -> (String, Value, Value) {
        let asked = questions::questions(&moment(rig), Some(rig));
        (
            questions::chat_system_prompt(&asked),
            questions::chat_schema(&asked),
            serde_json::json!(persona.personality),
        )
    }

    /// Whether a candidate's answers become a plan production would play.
    pub fn valid(raw: &str, rig: &RigStateSummary) -> bool {
        let Ok(answers) = serde_json::from_str::<Value>(strip_motion_json(raw)) else {
            return false;
        };
        let context = moment(rig);
        let asked = questions::questions(&context, Some(rig));
        let answered = asked
            .as_object()
            .is_some_and(|asked| asked.keys().all(|key| answers[key].is_object()));
        answered
            && matches!(
                parse_motion_decision(
                    &questions::plan_from_answers(&answers, &context, Some(rig)).to_string()
                ),
                Some(MotionDecision::Perform(_))
            )
    }

    #[test]
    fn a_candidate_must_answer_every_question() {
        let rig = myriad_merope::sanitize_rig_state(&serde_json::json!({})).unwrap();
        let answers = serde_json::json!({
            "feeling": {"choice": "calm", "confidence": 0.8},
            "own": {"noul": 0.9},
            "posture": {"choice": "neutral", "confidence": 0.7},
        });
        let asked = questions::questions(&moment(&rig), Some(&rig));
        let mut full = answers.clone();
        for key in asked.as_object().unwrap().keys() {
            if full[key].is_null() {
                full[key] = serde_json::json!({"choice": "none", "confidence": 0.1, "score": 0.0});
            }
        }
        assert!(self::valid(&full.to_string(), &rig));
        if asked.as_object().unwrap().len() > 3 {
            assert!(!self::valid(&answers.to_string(), &rig));
        }
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
