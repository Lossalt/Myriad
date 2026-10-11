//! Her acting judged by a decisions model (TypeSafe Jev): it does not write
//! text, it answers typed questions about the moment with how sure it is.
//! The answers become the same plan the chat director writes, so everything
//! after it (grounding, capability filter, playback) is shared.
//!
//! Measured on the motion cases (10-11): about 0.5 s a call against 4–9 s for
//! a chat model, feeling and gesture mostly right, wrong answers mostly unsure.

use myriad_merope::RigStateSummary;
use serde_json::{Map, Value, json};

use super::MotionContext;
use super::prompt::{offered_body_controls, offered_score_moves};

/// An answer less sure than this is left unacted; what she does not show
/// cannot be wrong.
const SURE: f64 = 0.3;
/// A beat lands on a fragment of the reply at most this long (score anchors).
const ANCHOR_CHARS: usize = 24;
/// At most this many fragments are offered to land a beat on.
const MAX_ANCHORS: usize = 8;

const FEELINGS: &[(&str, &str)] = &[
    ("calm", "even, nothing stirred"),
    ("warm", "fond, glad of them"),
    ("amused", "finds it funny, laughing"),
    ("puzzled", "honestly unsure, guessing"),
    ("shy", "flustered by praise or attention"),
    ("sulky", "playfully put out"),
    ("low", "sad or worn"),
    ("excited", "eager, lit up"),
];

const GESTURES: &[(&str, &str)] = &[
    ("nod", "agreement, acknowledgement"),
    ("shake", "refusal, disbelief, 'no no'"),
    ("glance", "a quick look away and back, checking or avoiding"),
    ("shrug", "who knows, casual dismissal"),
    ("beat", "a hand beat for emphasis while explaining"),
    ("sigh", "tired, relieved, exasperated"),
    ("bounce", "delight, eagerness"),
];

/// Whether `model` answers decisions rather than chat.
pub(in crate::services::agent) fn is_decisions_model(model: &str) -> bool {
    let model = model.trim().trim_start_matches('~');
    model.starts_with("typesafe/jev")
}

/// The decisions endpoint on the source a chat base URL names
/// (`https://openrouter.ai/api/v1` → `https://openrouter.ai/api/alpha/decisions`).
pub(in crate::services::agent) fn decisions_url(base_url: &str) -> Option<String> {
    let url = url::Url::parse(base_url).ok()?;
    Some(format!(
        "{}/api/alpha/decisions",
        url.origin().ascii_serialization()
    ))
}

/// What is asked about this moment, from what this face can do.
pub(super) fn questions(context: &MotionContext, state: Option<&RigStateSummary>) -> Value {
    let criteria = |pairs: &[(&str, &str)]| -> Value {
        pairs
            .iter()
            .map(|(name, means)| ((*name).to_owned(), json!(means)))
            .collect::<Map<_, _>>()
            .into()
    };
    let mut questions = Map::new();
    questions.insert(
        "feeling".into(),
        json!({"type": "choice",
        "instructions": "What she herself feels as she says this (not what she reports of others).",
        "criteria": criteria(FEELINGS)}),
    );
    questions.insert("own".into(), json!({"type": "noul",
        "instructions": "Is she showing her own feeling, rather than quoting or describing someone else's?",
        "criteria": {"true": "her own feeling, in her own words", "false": "quoting or describing someone else, or a story"}}));
    questions.insert("posture".into(), json!({"type": "choice",
        "instructions": "How she holds herself through it.",
        "criteria": {"closed": "drawn in, guarded", "neutral": "at ease", "open": "open, turned towards them", "leaning-in": "leaning in, engaged"}}));
    let controls = offered_body_controls(state);
    if controls.contains(&"headTilt") {
        questions.insert("tilt".into(), json!({"type": "score",
            "instructions": "How far her head tilts to one side while she says it.",
            "criteria": ["no tilt", "slight tilt, small yet plainly seen", "clear tilt, as in puzzlement or curiosity", "strong tilt, as far as it goes"]}));
    }
    if controls.contains(&"gazeHorizontal") && controls.contains(&"gazeVertical") {
        questions.insert("gaze".into(), json!({"type": "choice",
            "instructions": "Where her eyes mostly are while she says it.",
            "criteria": {"listener": "on the person", "away-thinking": "off to the side, thinking", "down": "lowered", "aside": "aside, avoiding their eyes"}}));
    }
    let offered = offered_score_moves(state);
    let mut gestures: Vec<(&str, &str)> = GESTURES
        .iter()
        .copied()
        .filter(|(name, _)| offered.contains(name))
        .collect();
    if !gestures.is_empty() {
        gestures.push(("none", "nothing beyond her ordinary talking"));
        questions.insert("gesture".into(), json!({"type": "choice",
            "instructions": "The one movement that goes and comes back by itself that best carries this, if any.",
            "criteria": criteria(&gestures)}));
        let anchors = anchors(context.response_text.as_deref());
        if anchors.len() > 1 {
            let pairs: Vec<(String, String)> = anchors
                .iter()
                .map(|a| (a.clone(), format!("as she says 「{a}」")))
                .collect();
            questions.insert(
                "anchor".into(),
                json!({"type": "choice",
                "instructions": "On which words of her reply does that movement land?",
                "criteria": pairs.into_iter().map(|(a, d)| (a, json!(d))).collect::<Map<_, _>>()}),
            );
        }
    }
    Value::Object(questions)
}

/// The moment as Jev reads it.
pub(super) fn state(context: &MotionContext, persona: Option<&str>) -> Value {
    json!({
        "she": persona.unwrap_or("a lively, warm companion"),
        "they said": context.user_text,
        "she replies": context.response_text.as_deref().unwrap_or("(not yet; she is about to answer)"),
        "her mood": context.mood.band_after,
    })
}

/// The reply's clauses a beat may land on, each its own first words.
pub(super) fn anchors(reply: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for clause in reply
        .unwrap_or_default()
        .split(|c: char| "。！？!?，,…\n；;".contains(c))
    {
        let clause: String = clause.trim().chars().take(ANCHOR_CHARS).collect();
        let clause = clause.trim().to_owned();
        if clause.chars().count() >= 2 && !out.contains(&clause) {
            out.push(clause);
        }
        if out.len() == MAX_ANCHORS {
            break;
        }
    }
    out
}

/// The answers as the chat director's plan (baseline, pose tiers, a beat).
pub(super) fn plan_from_answers(
    answers: &Value,
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> Value {
    let sure = |key: &str| -> Option<&str> {
        let answer = &answers[key];
        (answer["confidence"].as_f64().unwrap_or(0.0) >= SURE)
            .then(|| answer["choice"].as_str())
            .flatten()
    };
    let own = answers["own"]["noul"].as_f64().unwrap_or(1.0) >= 0.5;
    let feeling = if own {
        sure("feeling").unwrap_or("calm")
    } else {
        "calm"
    };
    let controls = offered_body_controls(state);
    let mut targets = Map::new();
    let mut set = |control: &str, tier: &str| {
        if controls.contains(&control) {
            targets.insert(control.to_owned(), json!(tier));
        }
    };
    let (expression, energy) = match feeling {
        "warm" => {
            set("mouthSmile", "slight");
            ("warm", 1.0)
        }
        "amused" => {
            set("mouthSmile", "clear");
            set("eyeSmile", "slight");
            ("warm", 1.1)
        }
        "shy" => {
            set("mouthSmile", "slight");
            ("warm", 0.9)
        }
        "sulky" => {
            set("mouthSmile", "-slight");
            ("tense", 1.0)
        }
        "low" => {
            set("mouthSmile", "-slight");
            set("headNod", "-slight");
            ("subdued", 0.8)
        }
        "excited" => {
            set("mouthSmile", "clear");
            set("eyeWide", "slight");
            ("warm", 1.2)
        }
        _ => ("steady", 0.95),
    };
    let posture = match sure("posture") {
        Some("closed") => "closed",
        Some("open") => "open",
        Some("leaning-in") => {
            set("torsoPitch", "slight");
            "open"
        }
        _ => "neutral",
    };
    // Which way is not asked: it alternates with the reply, as a person's does.
    let side = if context
        .response_text
        .as_deref()
        .unwrap_or(&context.user_text)
        .chars()
        .count()
        % 2
        == 0
    {
        ""
    } else {
        "-"
    };
    if let Some(score) = answers["tilt"]["score"].as_f64() {
        let tier = match score {
            s if s >= 2.5 => Some("strong"),
            s if s >= 1.5 => Some("clear"),
            s if s >= 0.6 => Some("slight"),
            _ => None,
        };
        if let Some(tier) = tier {
            set("headTilt", &format!("{side}{tier}"));
        }
    }
    match sure("gaze") {
        Some("away-thinking") => {
            set("gazeHorizontal", &format!("{side}slight"));
            set("gazeVertical", "slight");
        }
        Some("down") => set("gazeVertical", "-clear"),
        Some("aside") => set("gazeHorizontal", &format!("{side}clear")),
        _ => {}
    }
    let mut score = Vec::new();
    // Delight or a sigh for someone else's feeling is not hers to act.
    if let Some(kind) = sure("gesture")
        .filter(|kind| *kind != "none" && (own || !matches!(*kind, "bounce" | "sigh")))
    {
        let anchors = anchors(context.response_text.as_deref());
        let anchor = sure("anchor")
            .map(str::to_owned)
            .or_else(|| anchors.first().cloned());
        let amount = if matches!(feeling, "amused" | "excited") {
            "clear"
        } else {
            "slight"
        };
        let mut beat = json!({"move": {"kind": kind, "amount": amount}});
        match anchor {
            Some(text) if context.response_text.is_some() => beat["text"] = json!(text),
            _ => beat["atMs"] = json!(300),
        }
        score.push(beat);
    }
    json!({
        "baseline": {"expression": expression, "posture": posture, "motionEnergy": energy, "attention": 0.85,
            "pose": {"targets": targets, "transitionMs": 350, "holdMs": 0}},
        "cues": [],
        "score": score,
    })
}

/// Asks the decisions model about this moment; the plan as the chat director
/// would have written it, for the shared parser.
pub(super) async fn direct(
    context: &MotionContext,
    state: Option<&RigStateSummary>,
    persona: Option<&str>,
    source: &crate::config::ResolvedAiConfig,
    timeout: std::time::Duration,
) -> anyhow::Result<String> {
    let url =
        decisions_url(&source.base_url).ok_or_else(|| anyhow::anyhow!("no decisions endpoint"))?;
    let key = source
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no key for the acting source"))?;
    let body = json!({"model": source.model.trim(), "state": self::state(context, persona), "questions": questions(context, state)});
    let response = reqwest::Client::builder()
        .timeout(timeout)
        .build()?
        .post(url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let value: Value = response.json().await?;
    let usage = &value["usage"];
    crate::services::ai_cost_ledger::record_ai_tokens_from_attribution(
        &source.provider,
        &source.model,
        usage["input_tokens"].as_i64().unwrap_or(0) as i32,
        usage["output_tokens"].as_i64().unwrap_or(0) as i32,
        if status.is_success() {
            "completed"
        } else {
            "failed"
        },
        (!status.is_success()).then_some(status.as_str()),
    )
    .await;
    anyhow::ensure!(status.is_success(), "decisions call failed: {status}");
    Ok(plan_from_answers(&value["answers"], context, state).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::merope::motion::{
        MotionDecision, MotionPhase, parse_motion_decision,
    };

    fn context(reply: &str) -> MotionContext {
        MotionContext {
            user_id: 1,
            phase: MotionPhase::Delivery,
            mood: serde_json::from_value(
                json!({"before": 60.0, "after": 60.0, "bandBefore": "calm",
                "bandAfter": "calm", "delta": 0.0, "cause": "test", "revision": 1}),
            )
            .unwrap(),
            activity: "talking".into(),
            user_text: "你今天特别可爱。".into(),
            response_text: Some(reply.into()),
            previous_phrases: vec![],
            task_success: None,
            rig_state: None,
            motion_style: "even".into(),
        }
    }

    fn bust() -> RigStateSummary {
        myriad_merope::sanitize_rig_state(&json!({"speaking": true, "capabilities":
            ["head-body", "torso-volume", "independent-eyes", "mouth-shapes", "left-arm", "right-arm"]})).unwrap()
    }

    #[test]
    fn a_decisions_model_is_asked_on_its_source_decisions_endpoint() {
        assert!(
            is_decisions_model("typesafe/jev-1.13") && is_decisions_model(" ~typesafe/jev-latest")
        );
        assert!(!is_decisions_model("openai/gpt-6-luna"));
        assert_eq!(
            decisions_url("https://openrouter.ai/api/v1").as_deref(),
            Some("https://openrouter.ai/api/alpha/decisions")
        );
    }

    #[test]
    fn a_beat_may_land_on_each_clause_of_the_reply_as_written() {
        let anchors = anchors(Some("才、才没有呢……不过，谢谢你这么说。"));
        assert_eq!(anchors, ["才、才没有呢", "不过", "谢谢你这么说"]);
        assert!(
            anchors
                .iter()
                .all(|a| "才、才没有呢……不过，谢谢你这么说。".contains(a.as_str()))
        );
    }

    #[test]
    fn a_shy_answer_becomes_a_plan_the_shared_director_parser_takes() {
        let reply = "才、才没有呢……不过，谢谢你这么说。";
        let state = bust();
        let answers = json!({
            "feeling": {"type": "choice", "choice": "shy", "confidence": 0.98},
            "own": {"type": "noul", "noul": 0.93},
            "tilt": {"type": "score", "score": 0.76, "confidence": 0.49},
            "gesture": {"type": "choice", "choice": "glance", "confidence": 0.59},
            "anchor": {"type": "choice", "choice": "才、才没有呢", "confidence": 0.7},
            "posture": {"type": "choice", "choice": "closed", "confidence": 0.72},
            "gaze": {"type": "choice", "choice": "aside", "confidence": 0.48},
        });
        let raw = plan_from_answers(&answers, &context(reply), Some(&state)).to_string();
        assert!(super::super::semantic_valid(&raw, reply), "{raw}");
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(&raw) else {
            panic!("{raw}")
        };
        let baseline = plan.baseline.unwrap();
        assert_eq!(
            (baseline.expression.as_str(), baseline.posture.as_str()),
            ("warm", "closed")
        );
        let targets = baseline.pose.unwrap().targets;
        assert_eq!(targets["headTilt"].abs(), 0.35, "a slight tilt");
        assert_eq!(targets["gazeHorizontal"].abs(), 0.65, "eyes clearly aside");
        let value: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["score"][0]["text"], "才、才没有呢");
        assert_eq!(value["score"][0]["move"]["kind"], "glance");
    }

    #[test]
    fn someone_elses_delight_and_an_unsure_answer_are_not_acted() {
        let reply = "她说：“哈哈，我太开心了！”这是小说角色的台词。";
        let answers = json!({
            "feeling": {"type": "choice", "choice": "excited", "confidence": 0.8},
            "own": {"type": "noul", "noul": 0.2},
            "gesture": {"type": "choice", "choice": "bounce", "confidence": 0.78},
            "posture": {"type": "choice", "choice": "open", "confidence": 0.29},
        });
        let value = plan_from_answers(&answers, &context(reply), Some(&bust()));
        assert_eq!(value["baseline"]["expression"], "steady");
        assert_eq!(
            value["baseline"]["posture"], "neutral",
            "an unsure posture stays neutral"
        );
        assert_eq!(value["score"], json!([]));
    }
}
