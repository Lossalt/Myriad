//! Her acting judged by typed questions about the moment, each answered with
//! how sure the judgment model is: a decisions model (TypeSafe Jev) on its
//! own endpoint, a chat model as structured output. The answers become the
//! same plan the chat director writes, so everything after it (grounding,
//! capability filter, playback) is shared.
//!
//! Measured on the motion cases (10-12, a bust, two rounds): Jev about 0.6–1.2 s
//! a call; Luna answering about 2–8 s, none timed out; Luna writing the plan
//! instead 4–10 s, two of twenty timed out and one sulk smiled. Wrong answers
//! are mostly unsure, and unsure answers are not acted.

use myriad_merope::RigStateSummary;
use serde_json::{Map, Value, json};

use super::offered::{offered_body_controls, offered_score_moves};
use super::{MotionContext, control};

/// An answer less sure than this is left unacted; what she does not show
/// cannot be wrong.
pub(super) const SURE: f64 = 0.3;
/// Her feeling is taken as her own unless the model is this sure it is not:
/// quoting someone else scored 0.004–0.08 across Jev, Microsoft-Decision and
/// Luna, while her own laugh at them scored as low as 0.44 (10-12).
pub(super) const OWN: f64 = 0.25;
/// A beat lands on a fragment of the reply at most this long (score anchors).
const ANCHOR_CHARS: usize = 24;
/// At most this many fragments are offered to land a beat on.
const MAX_ANCHORS: usize = 8;

pub(super) const FEELINGS: &[(&str, &str)] = &[
    ("calm", "even, nothing stirred"),
    ("warm", "fond, glad of them"),
    ("amused", "finds it funny, laughing"),
    ("puzzled", "honestly unsure, guessing"),
    ("curious", "interested, wants to know more"),
    ("shy", "flustered by praise or attention"),
    ("sulky", "playfully put out"),
    ("low", "sad or worn"),
    ("excited", "eager, lit up"),
];

/// How a sentence is said: the speech phrase intents, as asked.
pub(super) const SAYING: &[(&str, &str)] = &[
    ("ask", "a real question or a guess put to them"),
    ("hesitate", "hesitating or puzzling it out, unsure"),
    (
        "tease",
        "ribbing them fondly, a joke or rhetorical jab at their expense",
    ),
    ("explain", "a turn of thought, earnest explaining"),
    ("check-in", "checking how they take it"),
    ("laugh", "she is actually laughing"),
    ("none", "none of these, said plainly"),
];
/// The faces someone may ask her for by name, as asked.
const ASKABLE: &[(&str, &str)] = &[
    ("maniac", "laughing like a maniac"),
    ("silly", "a silly, dopey face"),
    ("cry", "a crying face"),
    ("angry", "an angry face"),
    ("speechless", "a speechless, done-with-this face"),
    ("dizzy", "dizzy, seeing stars"),
    ("lovestruck", "lovestruck, heart eyes"),
    ("think", "a thinking face"),
];
/// At most this many sentences are asked about (speech phrases).
const MAX_SENTENCES: usize = 6;

const GESTURES: &[(&str, &str)] = &[
    ("nod", "agreement, acknowledgement"),
    ("shake", "refusal, disbelief, 'no no'"),
    ("glance", "a quick look away and back, checking or avoiding"),
    ("shrug", "who knows, casual dismissal"),
    ("beat", "a hand beat for emphasis while explaining"),
    ("sigh", "tired, relieved, exasperated"),
    ("bounce", "delight, eagerness"),
];

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
    for (index, sentence) in sentences(context.response_text.as_deref())
        .iter()
        .enumerate()
    {
        questions.insert(
            format!("say{index}"),
            json!({"type": "choice",
            "instructions": format!("How she says 「{sentence}」"),
            "criteria": criteria(SAYING)}),
        );
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
    if let Some(question) = asked_for_question(context, state) {
        questions.insert("askedFor".into(), question);
    }
    if let Some(question) = control::face_question(state, "while she says it") {
        questions.insert("face".into(), question);
    }
    questions.insert(
        "shift".into(),
        control::shift_question(state, "while she says it"),
    );
    questions.insert(
        "via".into(),
        control::via_question(state, "while she says it"),
    );
    Value::Object(questions)
}

/// A face asked for by name is hers to make or not: her reply says which.
/// Offered by what the face can make, speaking or not: a face she agrees to
/// make is made, even over her words.
pub(super) fn asked_for_question(
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> Option<Value> {
    let mut askable: Vec<(&str, &str)> = ASKABLE
        .iter()
        .copied()
        .filter(|(intent, _)| {
            state.is_none_or(|state| myriad_merope::cue_is_playable(&state.capabilities, intent))
        })
        .collect();
    if askable.is_empty() || context.user_text.trim().is_empty() || context.response_text.is_none()
    {
        return None;
    }
    askable.push((
        "none",
        "they asked for none of these, or her reply does not go along",
    ));
    let criteria: Map<String, Value> = askable
        .iter()
        .map(|(name, means)| ((*name).to_owned(), json!(means)))
        .collect();
    Some(json!({"type": "choice",
        "instructions": "Did they ask her to make one of these faces, and does her reply go along with making it?",
        "criteria": criteria}))
}

/// The face she agreed to make, played at once over a face that leaves it room.
pub(super) fn agreed_face(asked: Option<&str>) -> Vec<Value> {
    asked
        .map(|intent| {
            let sticker = matches!(intent, "maniac" | "silly" | "cry" | "dizzy" | "lovestruck" | "angry");
            json!({"intent": intent, "atMs": 0, "intensity": "strong", "tempo": "usual",
                "fadeInMs": if sticker { 180 } else { 100 }, "fadeOutMs": if sticker { 420 } else { 220 },
                "interrupt": "replace"})
        })
        .into_iter()
        .collect()
}

/// The moment the questions are about.
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

/// The reply's sentences, each as written with its closing mark, said once.
pub(super) fn sentences(reply: Option<&str>) -> Vec<String> {
    let reply = reply.unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = reply.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        // A quote or bracket closed right after the end of a sentence ends it.
        let closing = "。！？!?…\n".contains(c)
            || ("”」』）)\"'".contains(c)
                && current
                    .chars()
                    .rev()
                    .nth(1)
                    .is_some_and(|p| "。！？!?…".contains(p)));
        // A run of closing marks (……, ？！) and the quote or bracket they
        // close stay with their sentence.
        if closing
            && !chars
                .peek()
                .is_some_and(|next| "。！？!?…”」』）)\"'".contains(*next))
        {
            let sentence = current.trim().to_owned();
            current.clear();
            let count = sentence.chars().count();
            if (2..=120).contains(&count)
                && reply.matches(sentence.as_str()).count() == 1
                && !out.contains(&sentence)
            {
                out.push(sentence);
            }
        }
    }
    let rest = current.trim();
    if (2..=120).contains(&rest.chars().count()) && reply.matches(rest).count() == 1 {
        out.push(rest.to_owned());
    }
    out.truncate(MAX_SENTENCES);
    out
}

/// How far her head tilts, from the answer on its four-level scale.
pub(super) fn tilt_tier(answers: &Value) -> Option<&'static str> {
    match answers["tilt"]["score"].as_f64()? {
        s if s >= 2.5 => Some("strong"),
        s if s >= 1.5 => Some("clear"),
        s if s >= 0.6 => Some("slight"),
        _ => None,
    }
}

/// How a feeling shows on her face and head, and the energy she carries it with.
pub(super) fn feeling_pose(
    feeling: &str,
    side: &str,
    set: &mut impl FnMut(&str, &str),
) -> (&'static str, f64) {
    match feeling {
        "warm" => {
            set("mouthSmile", "slight");
            ("warm", 1.0)
        }
        "amused" => {
            set("mouthSmile", "clear");
            set("eyeSmile", "slight");
            ("warm", 1.1)
        }
        "curious" => {
            // Interest leans in, the head a little to one side.
            set("torsoPitch", "slight");
            set("headTilt", &format!("{side}slight"));
            ("steady", 1.05)
        }
        "shy" => {
            set("mouthSmile", "slight");
            ("warm", 0.9)
        }
        "sulky" => {
            // A playful huff turns her head away from them.
            set("headTurn", &format!("{side}clear"));
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
    }
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
    let own = answers["own"]["noul"].as_f64().unwrap_or(1.0) >= OWN;
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
    let (expression, energy) = feeling_pose(feeling, side, &mut set);
    let posture = match sure("posture") {
        Some("closed") => "closed",
        Some("open") => "open",
        Some("leaning-in") => {
            set("torsoPitch", "slight");
            "open"
        }
        _ => "neutral",
    };
    if let Some(tier) = tilt_tier(answers) {
        set("headTilt", &format!("{side}{tier}"));
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
    let face = control::lay_face(&mut targets, answers, state, own);
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
    let phrases: Vec<Value> = sentences(context.response_text.as_deref())
        .into_iter()
        .enumerate()
        .filter_map(|(index, text)| {
            let intent = sure(&format!("say{index}"))?;
            // A sentence she says plainly is "none" too, so the speech layer
            // does not act its question mark or laughter instead; a laugh
            // that is not hers is said plainly.
            let intent = if own || intent != "laugh" {
                intent
            } else {
                "none"
            };
            Some(json!({"text": text, "intent": intent}))
        })
        .collect();
    let asked = sure("askedFor").filter(|intent| *intent != "none");
    let cues = agreed_face(asked);
    let expression = if asked.is_some() {
        "steady"
    } else {
        expression
    };
    json!({
        "phrases": phrases,
        "baseline": {"expression": expression, "posture": posture, "motionEnergy": energy, "attention": 0.85,
            "pose": {"targets": targets, "transitionMs": 350, "holdMs": 0}, "face": face,
            "shift": sure("shift"), "via": sure("via").filter(|via| *via != "none")},
        "cues": cues,
        "score": score,
        "askedFor": asked,
    })
}

/// The moment and the questions about it: her body itself for her standing
/// acting when nobody is talking with her; otherwise her body itself for a
/// decisions model
/// (`direct`), and how she says her reply for a chat model, which would take
/// too long over every control.
fn asked(
    context: &MotionContext,
    state: Option<&RigStateSummary>,
    persona: Option<&str>,
    direct: bool,
) -> (Value, Value) {
    if context.phase == super::MotionPhase::Presence {
        // Standing acting waits for no reply: any judgment model has the
        // time to answer for every control; a decisions model chooses the
        // sequence as a combination.
        (
            control::standing_state(context, persona),
            if direct {
                control::standing_combined_questions(state)
            } else {
                control::standing_questions(context, state)
            },
        )
    } else if direct {
        (
            self::state(context, persona),
            control::control_questions(context, state),
        )
    } else {
        (self::state(context, persona), questions(context, state))
    }
}

fn plan(
    answers: &Value,
    context: &MotionContext,
    state: Option<&RigStateSummary>,
    direct: bool,
) -> Value {
    if context.phase == super::MotionPhase::Presence {
        control::standing_plan(answers, context, state)
    } else if direct {
        control::control_plan(answers, context, state)
    } else {
        plan_from_answers(answers, context, state)
    }
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
    let (asked_state, asked) = asked(context, state, persona, true);
    let body = json!({"model": source.model.trim(), "state": asked_state, "questions": asked});
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
    Ok(plan(&value["answers"], context, state, true).to_string())
}

/// The same questions put to a chat model, which answers each in the shape a
/// decisions model does; the plan then comes out of the same answers.
pub(super) fn chat_system_prompt(questions: &Value) -> String {
    let mut prompt = String::from(
        "You judge how a character acts at one moment of a chat: the input says who she is, what they said, what she replies and her mood. \
Answer every question below about her, as JSON. `confidence` is how sure you are, from 0 to 1: \
low when the moment does not show it plainly; what she does not show cannot be wrong. \
For a yes/no question, `noul` is the probability that the answer is true. \
For a scale, `score` runs from 0 (its first level) to the number of its last level.\n",
    );
    for (key, question) in questions.as_object().into_iter().flatten() {
        prompt.push_str(&format!(
            "\n{key}: {}\n",
            question["instructions"].as_str().unwrap_or_default()
        ));
        match &question["criteria"] {
            Value::Object(options) => {
                for (option, means) in options {
                    prompt.push_str(&format!(
                        "  - {option}: {}\n",
                        means.as_str().unwrap_or_default()
                    ));
                }
            }
            Value::Array(levels) => {
                for (level, means) in levels.iter().enumerate() {
                    prompt.push_str(&format!(
                        "  {level}: {}\n",
                        means.as_str().unwrap_or_default()
                    ));
                }
            }
            _ => {}
        }
    }
    prompt
}

/// The answer each question takes from a chat model, constrained.
pub(super) fn chat_schema(questions: &Value) -> Value {
    let unit = json!({"type": "number", "minimum": 0, "maximum": 1});
    let mut properties = Map::new();
    for (key, question) in questions.as_object().into_iter().flatten() {
        let answer = match question["type"].as_str() {
            Some("noul") => json!({"type": "object", "additionalProperties": false,
                "required": ["noul"], "properties": {"noul": unit}}),
            Some("score") => {
                let top = question["criteria"]
                    .as_array()
                    .map_or(0, |levels| levels.len().saturating_sub(1));
                json!({"type": "object", "additionalProperties": false,
                    "required": ["score", "confidence"],
                    "properties": {"score": {"type": "number", "minimum": 0, "maximum": top}, "confidence": unit}})
            }
            _ => {
                let options: Vec<&String> = question["criteria"]
                    .as_object()
                    .map(|options| options.keys().collect())
                    .unwrap_or_default();
                json!({"type": "object", "additionalProperties": false,
                    "required": ["choice", "confidence"],
                    "properties": {"choice": {"type": "string", "enum": options}, "confidence": unit}})
            }
        };
        properties.insert(key.clone(), answer);
    }
    let required: Vec<String> = properties.keys().cloned().collect();
    json!({"type": "object", "additionalProperties": false, "required": required, "properties": properties})
}

/// What the director is asked about a moment, as the semantic eval records it.
#[cfg(test)]
pub(in crate::services::agent) fn semantic_contract(context: &MotionContext) -> Value {
    let (asked_state, questions) = asked(context, context.rig_state.as_ref(), None, false);
    // The body and the reply decide which questions are asked; kept beside them.
    let input = json!({"moment": asked_state, "responseText": context.response_text,
        "rig": context.rig_state, "questions": questions});
    json!({"system": chat_system_prompt(&questions), "input": input.to_string(),
        "schema": chat_schema(&questions), "schemaName": "merope_acting_answers"})
}

/// Asks a chat model the decisions questions about this moment; the plan as
/// the chat director would have written it, for the shared parser.
pub(super) async fn direct_by_chat(
    context: &MotionContext,
    state: Option<&RigStateSummary>,
    persona: Option<&str>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> anyhow::Result<String> {
    let (asked_state, questions) = asked(context, state, persona, false);
    let raw = analyzer
        .analyze_json(
            &chat_system_prompt(&questions),
            &asked_state.to_string(),
            "merope_acting_answers",
            Some(&chat_schema(&questions)),
        )
        .await?;
    let answers: Value = serde_json::from_str(super::strip_motion_json(&raw))?;
    anyhow::ensure!(answers.is_object(), "answers must be an object");
    Ok(plan(&answers, context, state, false).to_string())
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
        use crate::config::is_decisions_model;
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
    fn each_sentence_is_asked_about_as_written_once() {
        let reply = "诶？这我怎么猜得到嘛……是拉面吗？";
        assert_eq!(
            sentences(Some(reply)),
            ["诶？", "这我怎么猜得到嘛……", "是拉面吗？"]
        );
        assert_eq!(
            sentences(Some("哈哈。哈哈。好")),
            Vec::<String>::new(),
            "a repeated sentence is not placed"
        );
        assert_eq!(
            sentences(Some("她说：“哈哈，我太开心了！”这是小说角色的台词。")),
            ["她说：“哈哈，我太开心了！”", "这是小说角色的台词。"],
            "a closing quote stays with what it closes"
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
            "say0": {"type": "choice", "choice": "hesitate", "confidence": 0.8},
            "say1": {"type": "choice", "choice": "none", "confidence": 0.9},
            "say2": {"type": "choice", "choice": "explain", "confidence": 0.2},
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
        assert_eq!(
            value["phrases"],
            json!([{"text": "才、才没有呢……", "intent": "hesitate"},
                {"text": "不过，谢谢你这么说。", "intent": "none"}]),
            "a plain sentence is said plainly; an unsure one is left to the speech layer"
        );
    }

    #[test]
    fn a_chat_model_is_asked_every_question_in_the_shape_a_decisions_model_answers() {
        let asked = questions(
            &context("才、才没有呢……不过，谢谢你这么说。"),
            Some(&bust()),
        );
        let schema = chat_schema(&asked);
        let keys: Vec<&String> = asked.as_object().unwrap().keys().collect();
        assert_eq!(
            schema["required"].as_array().unwrap().len(),
            keys.len(),
            "every question must be answered"
        );
        let feeling = &schema["properties"]["feeling"]["properties"];
        assert_eq!(
            feeling["choice"]["enum"].as_array().unwrap().len(),
            FEELINGS.len()
        );
        assert_eq!(schema["properties"]["own"]["required"], json!(["noul"]));
        assert_eq!(
            schema["properties"]["tilt"]["properties"]["score"]["maximum"], 3,
            "a scale runs to its last level"
        );
        let prompt = chat_system_prompt(&asked);
        assert!(prompt.contains("  - shy: flustered by praise or attention"));
        assert!(prompt.contains("  3: strong tilt, as far as it goes"));
    }

    #[test]
    fn curiosity_leans_in_with_a_slight_tilt() {
        let answers = json!({
            "feeling": {"type": "choice", "choice": "curious", "confidence": 0.8},
            "own": {"type": "noul", "noul": 0.95},
            "tilt": {"type": "score", "score": 0.0, "confidence": 0.7},
        });
        let value = plan_from_answers(&answers, &context("真的吗？学到哪一首了？"), Some(&bust()));
        let targets = &value["baseline"]["pose"]["targets"];
        assert_eq!(targets["torsoPitch"], "slight");
        assert_eq!(
            targets["headTilt"]
                .as_str()
                .unwrap()
                .trim_start_matches('-'),
            "slight",
            "a sure 'no tilt' does not undo it"
        );
    }

    #[test]
    fn a_laugh_the_model_is_unsure_is_her_own_is_still_acted() {
        // Microsoft-Decision on 「哈哈哈哈！」: amused, but `own` only 0.44.
        let answers = json!({
            "feeling": {"type": "choice", "choice": "amused", "confidence": 0.96},
            "own": {"type": "noul", "noul": 0.44},
            "say0": {"type": "choice", "choice": "laugh", "confidence": 0.62},
        });
        let value = plan_from_answers(
            &answers,
            &context("哈哈哈哈！你是想把迟到也冻住吗？"),
            Some(&bust()),
        );
        assert_eq!(value["baseline"]["expression"], "warm");
        assert_eq!(value["phrases"][0]["intent"], "laugh");
    }

    #[test]
    fn a_face_asked_for_by_name_is_hers_to_make_when_her_reply_goes_along() {
        let mut asked = context("好啊，看我的！");
        asked.user_text = "做一下狂笑".into();
        // Speaking: a face she agrees to make is made even over her words.
        let state = myriad_merope::sanitize_rig_state(&json!({"speaking": true, "capabilities":
            ["head-body", "independent-eyes", "mouth-shapes", "maniac-mouth"]}))
        .unwrap();
        let offered = questions(&asked, Some(&state));
        assert!(offered["askedFor"]["criteria"]["maniac"].is_string());
        assert!(
            offered["askedFor"]["criteria"]["cry"].is_null(),
            "a face this body cannot make is not offered"
        );
        let answers = json!({
            "feeling": {"type": "choice", "choice": "amused", "confidence": 0.9},
            "own": {"type": "noul", "noul": 0.9},
            "askedFor": {"type": "choice", "choice": "maniac", "confidence": 0.85},
        });
        let value = plan_from_answers(&answers, &asked, Some(&state));
        assert_eq!(value["cues"][0]["intent"], "maniac");
        assert_eq!(value["cues"][0]["interrupt"], "replace");
        assert_eq!(
            value["baseline"]["expression"], "steady",
            "room for the face"
        );
        assert!(super::super::semantic_valid(
            &value.to_string(),
            "好啊，看我的！"
        ));

        let declined = json!({"askedFor": {"type": "choice", "choice": "none", "confidence": 0.9}});
        assert_eq!(
            plan_from_answers(&declined, &asked, Some(&state))["cues"],
            json!([])
        );

        let mut no_reply = asked.clone();
        no_reply.response_text = None;
        assert!(
            questions(&no_reply, Some(&state))["askedFor"].is_null(),
            "whether she goes along waits for her reply"
        );
    }

    #[test]
    fn a_huff_turns_her_head_away() {
        let answers = json!({
            "feeling": {"type": "choice", "choice": "sulky", "confidence": 0.99},
            "own": {"type": "noul", "noul": 0.9},
        });
        let value = plan_from_answers(&answers, &context("哼，我可是等了好久的。"), Some(&bust()));
        let turn = value["baseline"]["pose"]["targets"]["headTurn"]
            .as_str()
            .unwrap();
        assert_eq!(turn.trim_start_matches('-'), "clear");
        assert_eq!(value["baseline"]["expression"], "tense");
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
