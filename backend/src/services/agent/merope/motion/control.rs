//! Her acting asked of a decisions model with the body itself (direct
//! control): every control this face has is its own question, answered with
//! the contract's tiers or rest; the expression, posture and energy are picked
//! from the contract; each sentence gets how it is said and which there-and-
//! back move lands on it. The answers are the plan: no table in between.
//!
//! A decisions model answers some forty questions in about a second (Jev
//! 0.7–1.1 s, Microsoft-Decision 1–2 s, 10-12); a chat model takes 5–12 s, so
//! it answers the shorter questions in `questions` instead.

use myriad_merope::RigStateSummary;
use serde_json::{Map, Value, json};

use super::MotionContext;
use super::offered::{offered_body_controls, offered_score_moves};
use super::questions::{FEELINGS, OWN, SAYING, SURE, agreed_face, asked_for_question, sentences};

const EXPRESSIONS: &[(&str, &str)] = &[
    ("withdrawn", "drawn in, avoiding, not opening up"),
    (
        "subdued",
        "held down but still present; restrained, earnest",
    ),
    ("steady", "her ordinary face"),
    ("warm", "relaxed, close, smiling"),
    (
        "tense",
        "irritable, taut, out of patience; brow down, eyes more open",
    ),
];
const POSTURES: &[(&str, &str)] = &[
    ("closed", "drawn in, not taking space"),
    ("neutral", "ordinary stance"),
    ("open", "open, closer, welcoming"),
];
const ENERGY: &[(&str, &str)] = &[
    ("low", "quiet, small movements"),
    ("usual", "her usual liveliness"),
    ("high", "lively, big movements"),
];

fn criteria(pairs: &[(&str, &str)]) -> Value {
    pairs
        .iter()
        .map(|(name, means)| ((*name).to_owned(), json!(means)))
        .collect::<Map<_, _>>()
        .into()
}

/// The tiers a control may take, each in words, and rest.
fn tier_options(control: &str) -> Value {
    let mut options = Map::new();
    options.insert("rest".into(), json!("where it rests; not directed"));
    for tier in myriad_merope::extent_tiers(control) {
        let (sign, strength) = match tier.strip_prefix('-') {
            Some(strength) => ("toward the negative end", strength),
            None => ("toward the positive end", tier.as_str()),
        };
        let how = match strength {
            "slight" => "a little, small yet plainly seen",
            "clear" => "clearly",
            _ => "as far as it goes",
        };
        options.insert(tier.clone(), json!(format!("{how}, {sign}")));
    }
    options.into()
}

/// Head, gaze, torso and arms: how she holds herself, not what she feels.
fn is_bearing(control: &str) -> bool {
    control.starts_with("head")
        || control.starts_with("gaze")
        || control.starts_with("torso")
        || control.contains("Arm")
}

/// What a decisions model is asked about this moment, from what this face can do.
pub(super) fn control_questions(context: &MotionContext, state: Option<&RigStateSummary>) -> Value {
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
    questions.insert(
        "expression".into(),
        json!({"type": "choice",
        "instructions": "Her face's standing expression while she says it.",
        "criteria": criteria(EXPRESSIONS)}),
    );
    questions.insert(
        "posture".into(),
        json!({"type": "choice",
        "instructions": "How she holds herself through it.",
        "criteria": criteria(POSTURES)}),
    );
    questions.insert(
        "energy".into(),
        json!({"type": "choice",
        "instructions": "How much she moves while she says it.",
        "criteria": criteria(ENERGY)}),
    );
    for control in offered_body_controls(state) {
        let means = myriad_merope::PERFORMANCE_BODY_CONTROLS
            .iter()
            .find(|c| c.0 == control)
            .map(|c| c.4)
            .unwrap_or_default();
        let hands = if control.contains("Arm") {
            " People talking use their hands: arms open a little when explaining or welcoming, draw in when shy or hurt, lift when excited."
        } else {
            ""
        };
        questions.insert(format!("body.{control}"), json!({"type": "choice",
            "instructions": format!("Her {control} while she says it ({means}) Image-left/right are as the viewer sees her.{hands} Leave it at rest unless this moment calls for it."),
            "criteria": tier_options(control)}));
    }
    let offered = offered_score_moves(state);
    let moves: Vec<(&str, &str)> = myriad_merope::PERFORMANCE_SCORE_MOVES
        .iter()
        .filter(|(name, _, _)| offered.contains(name))
        .map(|(name, _, means)| (*name, *means))
        .chain([("none", "no movement of its own on this sentence")])
        .collect();
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
        questions.insert(format!("move{index}"), json!({"type": "choice",
            "instructions": format!("Which movement that goes and comes back by itself lands as she says 「{sentence}」, if any. A person explaining or stressing a point often beats a hand; an unsure one shrugs."),
            "criteria": criteria(&moves)}));
    }
    if let Some(question) = asked_for_question(context, state) {
        questions.insert("askedFor".into(), question);
    }
    Value::Object(questions)
}

/// The answers as the plan, control by control.
pub(super) fn control_plan(
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
    let mut targets = Map::new();
    for control in offered_body_controls(state) {
        // Someone else's feeling is not hers to wear: only where she looks
        // and how she holds herself stay hers.
        if !own && !is_bearing(control) {
            continue;
        }
        if let Some(tier) = sure(&format!("body.{control}")).filter(|tier| *tier != "rest") {
            targets.insert(control.to_owned(), json!(tier));
        }
    }
    let energy = sure("energy").unwrap_or("usual");
    let amount = match energy {
        "low" => "slight",
        "high" => "strong",
        _ => "clear",
    };
    let mut score = Vec::new();
    let mut phrases = Vec::new();
    for (index, sentence) in sentences(context.response_text.as_deref())
        .into_iter()
        .enumerate()
    {
        if let Some(intent) = sure(&format!("say{index}")) {
            // A sentence said plainly is "none" too, so the speech layer does
            // not act its question mark or laughter instead.
            let intent = if own || intent != "laugh" {
                intent
            } else {
                "none"
            };
            phrases.push(json!({"text": sentence, "intent": intent}));
        }
        if let Some(kind) = sure(&format!("move{index}")).filter(|kind| *kind != "none") {
            // Delight or a sigh for someone else's feeling is not hers to act.
            if own || !matches!(kind, "bounce" | "sigh") {
                let anchor: String = sentence.chars().take(24).collect();
                score.push(json!({"text": anchor, "move": {"kind": kind, "amount": amount}}));
            }
        }
    }
    let asked = sure("askedFor").filter(|intent| *intent != "none");
    let expression = match (asked, own) {
        (None, true) => sure("expression").unwrap_or("steady"),
        _ => "steady",
    };
    json!({
        "phrases": phrases,
        "baseline": {"expression": expression, "posture": sure("posture").unwrap_or("neutral"),
            "motionEnergy": energy, "attention": "usual",
            "pose": {"targets": targets, "transitionMs": 350, "holdMs": 0}},
        "cues": agreed_face(asked),
        "score": score,
        "askedFor": asked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::merope::motion::{
        MotionDecision, MotionPhase, parse_motion_decision, semantic_valid,
    };

    fn context(user: &str, reply: &str) -> MotionContext {
        MotionContext {
            user_id: 1,
            phase: MotionPhase::Delivery,
            mood: serde_json::from_value(
                json!({"before": 60.0, "after": 60.0, "bandBefore": "calm",
                "bandAfter": "calm", "delta": 0.0, "cause": "test", "revision": 1}),
            )
            .unwrap(),
            activity: "talking".into(),
            user_text: user.into(),
            response_text: Some(reply.into()),
            previous_phrases: vec![],
            task_success: None,
            rig_state: None,
            motion_style: "even".into(),
        }
    }

    fn bust() -> RigStateSummary {
        myriad_merope::sanitize_rig_state(&json!({"speaking": true, "capabilities":
            ["head-body", "torso-volume", "independent-eyes", "mouth-shapes", "left-arm", "right-arm"]}))
        .unwrap()
    }

    #[test]
    fn every_control_this_face_has_is_asked_with_its_tiers_and_rest() {
        let state = bust();
        let asked = control_questions(&context("你好", "嗯，好。"), Some(&state));
        for control in offered_body_controls(Some(&state)) {
            let options = &asked[format!("body.{control}")]["criteria"];
            assert!(options["rest"].is_string(), "{control}");
            for tier in myriad_merope::extent_tiers(control) {
                assert!(options[&tier].is_string(), "{control} {tier}");
            }
        }
        assert!(
            asked["body.cry"].is_null(),
            "a face without tears is not asked"
        );
        assert_eq!(asked["body.eyeOpenLeft"]["criteria"]["slight"], Value::Null);
        assert!(
            asked["body.leftArmRaise"]["instructions"]
                .as_str()
                .unwrap()
                .contains("use their hands")
        );
        assert!(asked["say0"].is_object() && asked["move0"].is_object());
    }

    #[test]
    fn the_answers_are_the_plan_control_by_control() {
        let reply = "还有点难受。不过我愿意慢慢说。";
        let state = bust();
        let answers = json!({
            "own": {"noul": 0.95},
            "expression": {"choice": "subdued", "confidence": 0.8},
            "posture": {"choice": "closed", "confidence": 0.7},
            "energy": {"choice": "low", "confidence": 0.7},
            "body.headNod": {"choice": "-slight", "confidence": 0.8},
            "body.leftArmRaise": {"choice": "-slight", "confidence": 0.6},
            "body.browLift": {"choice": "-clear", "confidence": 0.2},
            "body.headTurn": {"choice": "rest", "confidence": 0.9},
            "say0": {"choice": "none", "confidence": 0.8},
            "move0": {"choice": "sigh", "confidence": 0.7},
        });
        let value = control_plan(&answers, &context("怎么了？", reply), Some(&state));
        let raw = value.to_string();
        assert!(semantic_valid(&raw, reply), "{raw}");
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(&raw) else {
            panic!("{raw}")
        };
        let targets = plan.baseline.unwrap().pose.unwrap().targets;
        assert_eq!(targets["headNod"], -0.35);
        assert!(targets["leftArmRaise"] < 0.0, "hands drawn in");
        assert!(
            !targets.contains_key("browLift"),
            "an unsure answer is not acted"
        );
        assert!(!targets.contains_key("headTurn"), "rest is not directed");
        assert_eq!(
            value["score"][0]["move"],
            json!({"kind": "sigh", "amount": "slight"})
        );
        assert_eq!(
            value["phrases"][0],
            json!({"text": "还有点难受。", "intent": "none"})
        );
    }

    #[test]
    fn someone_elses_laughter_keeps_her_bearing_but_not_her_face() {
        let reply = "她说：“哈哈，我太开心了！”这是小说角色的台词。";
        let answers = json!({
            "own": {"noul": 0.05},
            "expression": {"choice": "warm", "confidence": 0.9},
            "body.mouthSmile": {"choice": "strong", "confidence": 0.9},
            "body.eyeSmile": {"choice": "clear", "confidence": 0.9},
            "body.headTilt": {"choice": "slight", "confidence": 0.8},
            "body.leftArmRaise": {"choice": "slight", "confidence": 0.7},
            "say0": {"choice": "laugh", "confidence": 0.8},
            "move0": {"choice": "bounce", "confidence": 0.8},
        });
        let value = control_plan(&answers, &context("这句什么意思", reply), Some(&bust()));
        let targets = &value["baseline"]["pose"]["targets"];
        assert_eq!(value["baseline"]["expression"], "steady");
        assert!(targets["mouthSmile"].is_null() && targets["eyeSmile"].is_null());
        assert_eq!(targets["headTilt"], "slight");
        assert_eq!(targets["leftArmRaise"], "slight");
        assert_eq!(value["phrases"][0]["intent"], "none");
        assert_eq!(value["score"], json!([]));
    }

    #[test]
    fn a_face_she_agrees_to_make_is_played_over_a_steady_face() {
        let state = myriad_merope::sanitize_rig_state(&json!({"speaking": true, "capabilities":
            ["head-body", "mouth-shapes", "maniac-mouth"]}))
        .unwrap();
        let moment = context("做一下狂笑", "好啊，看我的！");
        assert!(
            control_questions(&moment, Some(&state))["askedFor"]["criteria"]["maniac"].is_string()
        );
        let answers = json!({
            "expression": {"choice": "warm", "confidence": 0.9},
            "askedFor": {"choice": "maniac", "confidence": 0.9},
        });
        let value = control_plan(&answers, &moment, Some(&state));
        assert_eq!(value["cues"][0]["intent"], "maniac");
        assert_eq!(value["baseline"]["expression"], "steady");
        assert_eq!(value["askedFor"], "maniac");
    }
}
