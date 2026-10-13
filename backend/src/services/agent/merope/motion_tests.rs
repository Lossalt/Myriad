//! Unit tests for `motion.rs`, kept beside it so the module stays readable.

use super::*;
use myriad_merope::{ChatPerformanceCue, PERFORMANCE_CUE_INTENTS, cue_survives_state};

#[test]
fn phases_have_stable_wire_names() {
    assert_eq!(MotionPhase::Reaction.as_str(), "reaction");
    assert_eq!(MotionPhase::Reaction.activity(), "thinking");
    assert_eq!(MotionPhase::Delivery.activity(), "talking");
    assert_eq!(MotionPhase::Mood.activity(), "idle");
    assert_eq!(
        serde_json::to_string(&MotionPhase::Delivery).unwrap(),
        "\"delivery\""
    );
}

#[test]
fn directive_wire_shape_is_camel_case_and_semantic_only() {
    let value = serde_json::to_value(PerformanceDirective {
        phase: MotionPhase::Reaction,
        mood_revision: 42,
        motion_style: "open".to_string(),
        phrases: vec![SpeechPhrase {
            text: "你觉得呢？".to_string(),
            intent: "check-in".to_string(),
        }],
        score: Vec::new(),
        loop_ms: None,
        plan: ChatPerformancePlan {
            baseline: None,
            cues: vec![myriad_merope::ChatPerformanceCue {
                intent: "listen".to_string(),
                at_ms: 0,
                intensity: 1.0,
                tempo: 1.0,
                fade_in_ms: 100,
                fade_out_ms: 200,
                interrupt: "if-lower".to_string(),
            }],
        },
    })
    .unwrap();
    assert_eq!(value["phase"], "reaction");
    assert_eq!(value["moodRevision"], 42);
    assert_eq!(value["motionStyle"], "open");
    assert!(value.pointer("/plan/cues/0/atMs").is_some());
    assert!(value.get("driver").is_none());
    assert_eq!(value["phrases"][0]["intent"], "check-in");
    assert_eq!(value["phrases"][0]["text"], "你觉得呢？");
}

fn rig(capabilities: &[&str], speaking: bool) -> RigStateSummary {
    myriad_merope::sanitize_rig_state(&serde_json::json!({
        "expression": "steady",
        "posture": "neutral",
        "owners": { "mouth": "idle", "expression": "idle", "gaze": "idle", "headBody": "idle" },
        "speaking": speaking,
        "capabilities": capabilities,
    }))
    .expect("summary")
}

/// Refining keeps exactly the cues that survive the rig state.
#[test]
fn refining_keeps_exactly_the_cues_that_survive_the_state() {
    for (capabilities, speaking) in [
        (&["head-body", "mouth-shapes"][..], false),
        (&["head-body", "cry-eye", "silly-eye"][..], false),
        (&["head-body", "maniac-mouth", "silly-mouth"][..], true),
        (&[][..], false),
    ] {
        let state = rig(capabilities, speaking);
        let surviving: Vec<&str> = PERFORMANCE_CUE_INTENTS
            .iter()
            .copied()
            .filter(|intent| cue_survives_state(&state, intent))
            .collect();
        assert!(!surviving.is_empty(), "{capabilities:?}");

        let plan = ChatPerformancePlan {
            baseline: None,
            cues: PERFORMANCE_CUE_INTENTS
                .iter()
                .map(|intent| ChatPerformanceCue {
                    intent: (*intent).to_string(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 120,
                    fade_out_ms: 200,
                    interrupt: "replace".to_string(),
                })
                .collect(),
        };
        let survived: Vec<String> = refine_performance_plan(plan, &state)
            .cues
            .into_iter()
            .map(|cue| cue.intent)
            .collect();
        assert_eq!(survived, surviving, "{capabilities:?} speaking={speaking}");
    }
}

#[test]
fn explicit_continue_is_not_a_plan() {
    assert_eq!(
        parse_motion_decision(r#"{"continue":true}"#),
        Some(MotionDecision::Continue)
    );
    assert_eq!(
        parse_motion_decision(r#"{"continue":true,"cues":[]}"#),
        Some(MotionDecision::Continue)
    );
    assert_eq!(
        parse_motion_decision("```json\n{\"continue\": true}\n```"),
        Some(MotionDecision::Continue)
    );
}

#[test]
fn empty_object_is_invalid_not_continue() {
    assert_eq!(parse_motion_decision("{}"), None);
    assert_eq!(parse_motion_decision(r#"{"cues":[]}"#), None);
    assert_eq!(parse_motion_decision(r#"{"continue":false}"#), None);
}

#[test]
fn illegal_baseline_without_cues_is_invalid() {
    assert_eq!(
        parse_motion_decision(
            r#"{"baseline":{"expression":"angry","posture":"attack"},"cues":[]}"#
        ),
        None
    );
}

#[test]
fn legal_plan_is_perform() {
    let decision = parse_motion_decision(
        r#"{"cues":[{"intent":"listen","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"if-lower"}]}"#,
    );
    match decision {
        Some(MotionDecision::Perform(plan)) => {
            assert_eq!(plan.cues.len(), 1);
            assert_eq!(plan.cues[0].intent, "listen");
        }
        other => panic!("expected perform, got {other:?}"),
    }
}

#[test]
fn continue_with_a_plan_is_rejected() {
    assert_eq!(
        parse_motion_decision(r#"{"continue":true,"cues":[{"intent":"listen"}]}"#),
        None
    );
    assert_eq!(
        parse_motion_decision(
            r#"{"continue":true,"baseline":{"expression":"warm","posture":"open","motionEnergy":1,"attention":0.8}}"#
        ),
        None
    );
}

/// Chat speaks plain prose; the director observes it on a separate task.
#[test]
fn streaming_chat_refines_actual_delivery_without_delaying_text() {
    let src = include_str!("../process_chat.rs");
    assert!(src.contains("let performance = None;"));
    let chat = src
        .find("stream_strict_lite_chat_response")
        .expect("chat lite call");
    let local_reaction = src
        .find("local_directive(&reaction_context)")
        .expect("local reaction");
    assert!(
        local_reaction < chat,
        "Chat must react before the reply stream starts"
    );
    assert!(src.contains("spawn_chat_motion_refinement("));
    let streaming = include_str!("../confirmation_and_tasks/chat_stream.rs");
    assert!(streaming.contains("emit_chat_delta(&tx, delta, speech_delivery.as_ref()).await"));
    assert!(!streaming.contains("SpeechDeliveryStream"));
    assert!(!include_str!("../chat_prompt.rs").contains("[[delivery:"));
}

#[test]
fn immediate_reaction_precedes_text_and_landing_cannot_delay_stream_close() {
    let src = include_str!("../process_chat.rs");
    let floor = src.find("local_directive(&reaction_context)").unwrap();
    let delivery = src.find("local_directive(&delivery_context)").unwrap();
    assert!(floor < delivery);
    let chat = src[floor..]
        .find("stream_strict_lite_chat_response")
        .unwrap()
        + floor;
    // What they say in chat is already in the conversation she reads: no
    // second copy of it is kept as something that happened.
    assert!(!src.contains("note_chat_diary"));
    assert!(chat < delivery);
    let finish = src
        .find("response_agent::finish_stream(&progress_tx)")
        .unwrap();
    assert!(finish < delivery);
    let stop = src.find("guard.stop().await").unwrap();
    assert!(finish < stop && stop < delivery);
    let overlay = include_str!("../motion_overlay.rs");
    assert!(overlay.contains("self.task.abort();"));
    assert!(src[delivery..].contains("performance.plan.cues.clear()"));
}

#[test]
fn chat_director_phrase_only_decision_needs_no_replacement_pose() {
    assert_eq!(
        parse_motion_decision(r#"{"phrases":[{"text":"你觉得呢？","intent":"check-in"}]}"#),
        Some(MotionDecision::Perform(ChatPerformancePlan::default()))
    );
    for invalid in [
        r#"{"phrases":[{"text":"你觉得呢？","intent":"driver"}]}"#,
        r#"{"baseline":{},"phrases":[{"text":"你觉得呢？","intent":"ask"}]}"#,
        r#"{"continue":true,"phrases":[{"text":"你觉得呢？","intent":"ask"}]}"#,
    ] {
        assert_eq!(parse_motion_decision(invalid), None);
    }
}

/// MOTION_TIMEOUT >= 8s; MOTION_TOTAL_TIMEOUT > MOTION_TIMEOUT.
/// Request paths must not `handle.await.ok().flatten()`.
#[test]
fn the_director_budget_clears_the_observed_success_latency() {
    assert!(MOTION_TIMEOUT >= Duration::from_secs(8));
    assert!(MOTION_TOTAL_TIMEOUT > MOTION_TIMEOUT);
    let src = concat!(
        include_str!("../process_and_recipe.rs"),
        include_str!("../process_chat.rs"),
        include_str!("../process_work.rs")
    );
    assert!(
        !src.contains("handle.await.ok().flatten()"),
        "no request path may block on the director's budget"
    );
}

#[test]
fn a_director_with_no_answer_still_leaves_the_round_something_to_play() {
    let plan = local_performance_plan(
        MotionPhase::Delivery,
        &MoodTransition {
            before: 50.0,
            after: 50.0,
            arousal_before: 48.0,
            arousal_after: 48.0,
            band_before: "calm".to_string(),
            band_after: "calm".to_string(),
            delta: 0.0,
            cause: "test".to_string(),
            revision: 1,
        },
        None,
        "even",
    );
    assert!(!plan_is_empty(&plan));
}

#[test]
fn hidden_face_skips_motion() {
    let hidden = myriad_merope::sanitize_rig_state(&serde_json::json!({
        "pageVisible": false,
        "faceVisible": true
    }))
    .unwrap();
    assert!(face_is_hidden(Some(&hidden)));
    let no_face = myriad_merope::sanitize_rig_state(&serde_json::json!({
        "pageVisible": true,
        "faceVisible": false
    }))
    .unwrap();
    assert!(face_is_hidden(Some(&no_face)));
    assert!(!face_is_hidden(None));
}

#[test]
fn a_score_alone_is_a_performance_and_its_moves_follow_the_body() {
    // Only beats, no baseline: still new direction, e.g. while listening.
    assert!(matches!(
        parse_motion_decision(r#"{"score":[{"atMs":300,"move":{"kind":"nod","count":2}}]}"#),
        Some(MotionDecision::Perform(_))
    ));
    assert!(
        parse_motion_decision(r#"{"score":[{"atMs":300,"move":{"kind":"moonwalk"}}]}"#).is_none()
    );
    assert!(
        parse_motion_decision(r#"{"continue":true,"score":[{"atMs":0,"move":{"kind":"nod"}}]}"#)
            .is_none()
    );

    let head_only = rig(&["head-body"], false);
    let kinds = offered_score_moves(Some(&head_only));
    assert!(kinds.contains(&"nod") && kinds.contains(&"shrug"));
    assert!(!kinds.contains(&"beat"), "no hand beat without arms");
    assert!(
        !kinds.contains(&"glance"),
        "no eye glance without independent eyes"
    );
}

#[test]
fn standing_acting_is_directed_on_time_and_loops() {
    assert_eq!(MotionPhase::Presence.as_str(), "presence");
    assert_eq!(MotionPhase::Presence.activity(), "idle");
}

#[test]
fn a_face_she_agreed_to_make_plays_even_while_she_speaks() {
    let cue = ChatPerformanceCue {
        intent: "maniac".into(),
        at_ms: 0,
        intensity: 1.3,
        tempo: 1.0,
        fade_in_ms: 180,
        fade_out_ms: 420,
        interrupt: "replace".into(),
    };
    let plan = ChatPerformancePlan {
        baseline: None,
        cues: vec![cue.clone()],
    };
    let speaking = rig(&["head-body", "mouth-shapes", "maniac-mouth"], true);
    let filtered = refine_performance_plan(plan.clone(), &speaking);
    assert!(filtered.cues.is_empty(), "the state filter alone drops it");
    let kept = keep_agreed_face(filtered, Some(cue.clone()), &speaking);
    assert_eq!(kept.cues[0].intent, "maniac");
    let no_mouth = rig(&["head-body"], true);
    let dropped = keep_agreed_face(
        refine_performance_plan(plan, &no_mouth),
        Some(cue),
        &no_mouth,
    );
    assert!(dropped.cues.is_empty(), "not a face this body can make");
}

#[test]
fn offline_her_standing_acting_still_shows_what_she_is_doing() {
    for way in [
        "reading",
        "listening",
        "daydreaming",
        "looking-about",
        "resting",
    ] {
        let context = MotionContext {
            user_id: 1,
            phase: MotionPhase::Presence,
            mood: serde_json::from_value(serde_json::json!({"before": 60.0, "after": 60.0,
                "bandBefore": "calm", "bandAfter": "calm", "delta": 0.0, "cause": "standing", "revision": 1}))
            .unwrap(),
            activity: format!("{way} on her own"),
            user_text: String::new(),
            response_text: None,
            previous_phrases: Vec::new(),
            task_success: None,
            rig_state: Some(rig(&["head-body", "independent-eyes", "mouth-shapes"], false)),
            motion_style: "even".into(),
        };
        let floor = standing_floor(&context, way).expect(way);
        assert_eq!(floor.loop_ms, Some(STANDING_LOOP_MS), "{way}");
        assert!(!floor.score.is_empty(), "{way} has beats");
    }
}
