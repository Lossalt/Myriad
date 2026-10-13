//! Her standing acting when the director cannot be reached (offline): what
//! she is doing on her own, by its kind, shown by fixed rules as a score of
//! beats on time that plays again each loop. The director's own standing
//! acting is `control::standing_questions`.

use myriad_merope::RigStateSummary;
use serde_json::{Map, Value, json};

use super::MotionContext;
use super::control::{STANDING_LOOP_MS, activity_seed, next_seed};
use super::offered::{offered_body_controls, offered_score_moves};
use super::questions::{feeling_pose, tilt_tier};

/// How what she is doing on her own shows on her (standing acting); the
/// shapes are in `plan_from_answers`, and the test walks every one.
#[cfg(test)]
const WAYS: &[(&str, &str)] = &[
    ("reading", "eyes down, moving along lines"),
    ("listening", "eyes soft, small nods with the music"),
    ("daydreaming", "gaze drifting off, lost in thought"),
    ("looking-about", "looking about, browsing around"),
    ("resting", "still and drowsy, eyes heavy"),
];
/// Her standing acting for what she is doing, shown the `way` its kind shows
/// (`WAYS`), with her mood as it stands; no model is asked.
pub(in crate::services::agent) fn standing_floor(
    way: &str,
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> Value {
    let feeling = match context.mood.band_after.as_str() {
        "floor" | "sad" | "low" => "low",
        "excited" | "high" => "warm",
        _ => "calm",
    };
    let answers = json!({
        "feeling": {"choice": feeling, "confidence": 1.0},
        "way": {"choice": way, "confidence": 1.0},
        "liveliness": {"choice": "gentle", "confidence": 1.0},
    });
    plan_from_answers(&answers, context, state)
}

/// The answers as her standing acting: a baseline and a score of beats on
/// time that plays again each loop, uneven within it so the repeat does not
/// show.
fn plan_from_answers(
    answers: &Value,
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> Value {
    let sure = |key: &str| super::control::sure(answers, key);
    let controls = offered_body_controls(state);
    let moves = offered_score_moves(state);
    // Uneven, but the same for the same activity: one direction serves everyone.
    let mut seed = activity_seed(&context.activity);
    let side = if seed % 2 == 0 { "" } else { "-" };
    let mut jitter = |spread: u64| -> u64 { u64::from(next_seed(&mut seed)) % (spread * 2 + 1) };
    let other = if side.is_empty() { "-" } else { "" };
    // A tier toward her side of the look this time.
    let flip = |tier: &str| -> String {
        match (side.is_empty(), tier.strip_prefix('-')) {
            (true, _) => tier.to_owned(),
            (false, Some(toward)) => toward.to_owned(),
            (false, None) => format!("-{tier}"),
        }
    };
    let mut targets = Map::new();
    let mut set = |control: &str, tier: &str| {
        if controls.contains(&control) {
            targets.insert(control.to_owned(), json!(tier));
        }
    };
    let feeling = sure("feeling").unwrap_or("calm");
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
    let (pace, amount, liveliness) = match sure("liveliness") {
        Some("still") => (1.4, "slight", 0.85),
        Some("lively") => (0.75, "clear", 1.1),
        _ => (1.0, "slight", 1.0),
    };
    let way = sure("way");
    match way {
        Some("reading") => {
            set("gazeVertical", "-clear");
            set("headNod", "-slight");
        }
        Some("listening") => {
            set("eyeSmile", "slight");
            set("gazeVertical", "-slight");
        }
        Some("daydreaming") => {
            set("gazeHorizontal", &format!("{side}slight"));
            set("gazeVertical", "slight");
        }
        Some("resting") => {
            set("eyeOpenLeft", "-slight");
            set("eyeOpenRight", "-slight");
            set("gazeVertical", "-slight");
        }
        _ => {}
    }
    let pose_beat = |at: u64,
                     pose: &[(&str, String)],
                     transition: u64,
                     hold: u64|
     -> Option<Value> {
        let offered: Map<String, Value> = pose
            .iter()
            .filter(|(control, _)| controls.contains(control))
            .map(|(control, tier)| ((*control).to_owned(), json!(tier)))
            .collect();
        (!offered.is_empty()).then(|| {
            json!({"atMs": at, "pose": {"targets": offered, "transitionMs": transition, "holdMs": hold}})
        })
    };
    let move_beat = |at: u64, kind: &str, extra: Value| -> Option<Value> {
        moves.contains(&kind).then(|| {
            let mut beat = json!({"atMs": at, "move": {"kind": kind, "amount": amount}});
            if let Value::Object(more) = extra {
                beat["move"].as_object_mut().unwrap().extend(more);
            }
            beat
        })
    };
    let mut score: Vec<Option<Value>> = Vec::new();
    let every = |base: u64| (base as f64 * pace) as u64;
    let mut at = jitter(400);
    match way {
        Some("reading") => {
            // Line after line: eyes to the start of a line, then slowly along it.
            let line = every(2_600);
            while at + line < STANDING_LOOP_MS && score.len() < 12 {
                score.push(pose_beat(
                    at,
                    &[("gazeHorizontal", "-slight".into())],
                    200,
                    300,
                ));
                score.push(pose_beat(
                    at + 350,
                    &[("gazeHorizontal", "slight".into())],
                    line - 600,
                    300,
                ));
                at += line + jitter(500);
            }
            score.push(move_beat(
                STANDING_LOOP_MS - 2_500 - jitter(800),
                "nod",
                json!({"tempo": "slow"}),
            ));
        }
        Some("listening") => {
            while at < STANDING_LOOP_MS - 1_500 {
                let count = if amount == "clear" { 3 } else { 2 };
                score.push(move_beat(
                    at,
                    "nod",
                    json!({"count": count, "tempo": "slow"}),
                ));
                at += every(4_200) + jitter(1_000);
            }
        }
        Some("daydreaming") => {
            let drifts = [
                ("slight", "slight"),
                ("-slight", "slight"),
                ("slight", "-slight"),
            ];
            let mut index = 0;
            while at < STANDING_LOOP_MS - 3_000 {
                let (horizontal, vertical) = drifts[index % drifts.len()];
                let horizontal = flip(horizontal);
                score.push(pose_beat(
                    at,
                    &[
                        ("gazeHorizontal", horizontal),
                        ("gazeVertical", vertical.into()),
                    ],
                    1_500,
                    3_500,
                ));
                index += 1;
                at += every(5_500) + jitter(1_500);
            }
            if matches!(feeling, "low" | "calm") {
                score.push(move_beat(
                    9_000 + jitter(2_000),
                    "sigh",
                    json!({"tempo": "slow"}),
                ));
            }
        }
        Some("looking-about") => {
            let directions = [
                "left",
                "up-right",
                "right",
                "down-left",
                "up-left",
                "down-right",
            ];
            let mut index = jitter(3) as usize;
            while at < STANDING_LOOP_MS - 1_000 && score.len() < 10 {
                score.push(move_beat(
                    at,
                    "glance",
                    json!({"direction": directions[index % directions.len()]}),
                ));
                index += 1;
                at += every(2_800) + jitter(800);
            }
            score.push(pose_beat(
                6_000 + jitter(1_500),
                &[("headTurn", format!("{side}slight"))],
                900,
                2_500,
            ));
            score.push(pose_beat(
                14_000 + jitter(1_500),
                &[("headTurn", format!("{other}slight"))],
                900,
                2_500,
            ));
        }
        Some("resting") => {
            score.push(move_beat(
                7_000 + jitter(1_500),
                "sigh",
                json!({"tempo": "slow"}),
            ));
            score.push(move_beat(
                15_000 + jitter(1_500),
                "blink",
                json!({"tempo": "slow"}),
            ));
        }
        _ => {}
    }
    let mut score: Vec<Value> = score.into_iter().flatten().collect();
    score.sort_by_key(|beat| beat["atMs"].as_u64().unwrap_or(0));
    score.truncate(myriad_merope::SCORE_MAX_BEATS as usize);
    json!({
        "baseline": {"expression": expression, "posture": posture, "motionEnergy": energy * liveliness, "attention": 0.6,
            "pose": {"targets": targets, "transitionMs": 800, "holdMs": 0}},
        "cues": [],
        "score": score,
        "loopMs": STANDING_LOOP_MS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::merope::motion::{
        MotionDecision, MotionPhase, parse_motion_decision,
    };

    fn context(activity: &str) -> MotionContext {
        MotionContext {
            user_id: 1,
            phase: MotionPhase::Presence,
            mood: serde_json::from_value(
                json!({"before": 60.0, "after": 60.0, "bandBefore": "calm",
                "bandAfter": "calm", "delta": 0.0, "cause": "test", "revision": 1}),
            )
            .unwrap(),
            activity: activity.into(),
            user_text: String::new(),
            response_text: None,
            previous_phrases: vec![],
            task_success: None,
            rig_state: None,
            motion_style: "even".into(),
        }
    }

    fn bust() -> RigStateSummary {
        myriad_merope::sanitize_rig_state(&json!({"speaking": false, "capabilities":
            ["head-body", "torso-volume", "independent-eyes", "mouth-shapes", "left-arm", "right-arm"]})).unwrap()
    }

    #[test]
    fn her_standing_acting_is_a_looped_score_for_each_way_it_shows() {
        let context = context("listening to 夜に駆ける");
        let state = bust();
        assert_eq!(
            standing_floor("listening", &context, Some(&state))["loopMs"],
            STANDING_LOOP_MS
        );
        for (way, _) in WAYS {
            let answers = json!({
                "feeling": {"type": "choice", "choice": "warm", "confidence": 0.8},
                "way": {"type": "choice", "choice": way, "confidence": 0.9},
                "liveliness": {"type": "choice", "choice": "gentle", "confidence": 0.7},
            });
            let value = plan_from_answers(&answers, &context, Some(&state));
            let raw = value.to_string();
            assert!(
                matches!(
                    parse_motion_decision(&raw),
                    Some(MotionDecision::Perform(_))
                ),
                "{way}: {raw}"
            );
            assert_eq!(value["loopMs"], STANDING_LOOP_MS);
            let at: Vec<u64> = value["score"]
                .as_array()
                .unwrap()
                .iter()
                .map(|beat| beat["atMs"].as_u64().unwrap())
                .collect();
            assert!(!at.is_empty(), "{way} has beats");
            assert!(at.windows(2).all(|w| w[0] <= w[1]), "{way}: in order");
            assert!(
                at.iter().all(|&ms| ms < STANDING_LOOP_MS),
                "{way}: within the loop"
            );
            assert_eq!(
                plan_from_answers(&answers, &context, Some(&state)),
                value,
                "{way}: the same for everyone watching"
            );
        }
    }

    #[test]
    fn reading_looks_down_and_moves_along_lines() {
        let context = context("reading 小王子");
        let answers = json!({
            "feeling": {"type": "choice", "choice": "calm", "confidence": 0.8},
            "way": {"type": "choice", "choice": "reading", "confidence": 0.9},
        });
        let value = plan_from_answers(&answers, &context, Some(&bust()));
        assert_eq!(
            value["baseline"]["pose"]["targets"]["gazeVertical"],
            "-clear"
        );
        let along = value["score"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|beat| beat["pose"]["targets"]["gazeHorizontal"] == "slight")
            .count();
        assert!(along >= 4, "a run of lines in each loop: {along}");
    }

    #[test]
    fn a_face_without_the_controls_gets_no_beats_it_cannot_play() {
        let context = context("looking around the site");
        let mouth_only = myriad_merope::sanitize_rig_state(
            &json!({"speaking": false, "capabilities": ["mouth-shapes"]}),
        )
        .unwrap();
        let answers = json!({
            "way": {"type": "choice", "choice": "looking-about", "confidence": 0.9},
        });
        let value = plan_from_answers(&answers, &context, Some(&mouth_only));
        assert_eq!(value["score"], json!([]));
    }
}
