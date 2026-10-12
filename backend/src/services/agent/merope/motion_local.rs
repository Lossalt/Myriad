//! Deterministic acting floor: her state, not what anyone said.
//!
//! The judgment model is her only director; it reads the words. The floor
//! plays at once and stands when the director has nothing: it shows her mood
//! (band, how it moved and why), whether a task worked, and whether she spoke
//! first, and never reads meaning into text. `local_performance_plan` uses
//! phase, mood, task_success and motion_style (no activity). Chat publishes
//! `local_directive` before the reply stream; the director's refinement uses
//! `fallback_to_local: false`. `direct_motion` is the no-publisher path.
//! Amplitudes clamp 0.2..=1.4 (performance contract, not ambient).
//! A floor nobody can see is the same as no floor.

use myriad_merope::{ChatPerformanceBaseline, ChatPerformanceCue, ChatPerformancePlan};

use super::MotionPhase;
use super::state::MoodTransition;

/// Mood moves smaller than this are noise, not a change of bearing.
const MOOD_NUDGE: f64 = 8.0;

const EXPRESSION_LADDER: [&str; 4] = ["withdrawn", "subdued", "steady", "warm"];

pub fn local_performance_plan(
    phase: MotionPhase,
    mood: &MoodTransition,
    task_success: Option<bool>,
    motion_style: &str,
) -> ChatPerformancePlan {
    let baseline = local_baseline(phase, mood, motion_style);
    let cues: Vec<ChatPerformanceCue> = local_cue(phase, mood, task_success, &baseline)
        .into_iter()
        .collect();
    ChatPerformancePlan {
        baseline: Some(baseline),
        cues,
    }
}

fn local_baseline(
    phase: MotionPhase,
    mood: &MoodTransition,
    motion_style: &str,
) -> ChatPerformanceBaseline {
    ChatPerformanceBaseline {
        expression: baseline_expression(&mood.band_after, mood.delta).to_string(),
        posture: baseline_posture(motion_style, &mood.band_after).to_string(),
        motion_energy: baseline_energy(motion_style, &mood.band_after),
        attention: phase_attention(phase),
        pose: None,
        face: None,
    }
}

/// Band sets the rung; a decisive swing this round moves it one step.
///
/// `tense` answers before the ladder because it is not on it. `mood_band`
/// splits `sad` from `tense` on arousal. The ladder is a valence axis;
/// only `subdued` / `withdrawn` close the eyes.
fn baseline_expression(band: &str, delta: f64) -> &'static str {
    if band == "tense" {
        return "tense";
    }
    let rung = match band {
        "floor" => 0i32,
        "sad" | "low" => 1,
        "excited" | "high" => 3,
        _ => 2,
    };
    let step = if delta >= MOOD_NUDGE {
        1
    } else if delta <= -MOOD_NUDGE {
        -1
    } else {
        0
    };
    EXPRESSION_LADDER[(rung + step).clamp(0, 3) as usize]
}

fn baseline_posture(motion_style: &str, band: &str) -> &'static str {
    if band == "floor" {
        return "closed";
    }
    match motion_style {
        "restrained" => "closed",
        "open" => "open",
        _ if band == "excited" || band == "high" => "open",
        _ => "neutral",
    }
}

fn baseline_energy(motion_style: &str, band: &str) -> f32 {
    let style: f32 = match motion_style {
        "restrained" => 0.6,
        "open" => 1.2,
        _ => 0.9,
    };
    let band_scale: f32 = match band {
        "floor" => 0.85,
        "sad" | "low" => 0.92,
        "tense" => 1.05,
        "excited" | "high" => 1.12,
        _ => 1.0,
    };
    (style * band_scale).clamp(0.2, 1.4)
}

/// Scales ambient wander (`1 - 0.3 * attention`); not a face budget.
fn phase_attention(phase: MotionPhase) -> f32 {
    match phase {
        MotionPhase::Reaction => 0.9,
        MotionPhase::Delivery => 0.72,
        MotionPhase::Outcome => 0.6,
        MotionPhase::Proactive => 0.55,
        MotionPhase::Mood | MotionPhase::Presence => 0.4,
    }
}

/// One beat per phase, from what is known without reading the words: why her
/// mood moved, how a task went, that she spoke first. A mood shift is a change
/// of bearing, not an event, so it moves the baseline and plays nothing.
fn local_cue(
    phase: MotionPhase,
    mood: &MoodTransition,
    task_success: Option<bool>,
    baseline: &ChatPerformanceBaseline,
) -> Option<ChatPerformanceCue> {
    let intent = match phase {
        // Text submission is already a completed turn. A small acknowledgement
        // is honest; "listen" falsely claims an ongoing listener state that
        // this path cannot observe.
        MotionPhase::Reaction if mood.cause == "user_praise" || mood.delta >= MOOD_NUDGE => {
            "delight"
        }
        MotionPhase::Reaction if mood.cause == "user_scold" => "speechless",
        MotionPhase::Reaction | MotionPhase::Delivery => "respond",
        MotionPhase::Outcome => match task_success {
            Some(true) => "delight",
            _ => "respond",
        },
        MotionPhase::Proactive => "notify",
        MotionPhase::Mood | MotionPhase::Presence => return None,
    };
    let energy = baseline.motion_energy;
    let (fade_in_ms, fade_out_ms) = match intent {
        "delight" => (105, 460),
        "notify" => (110, 380),
        "speechless" => (115, 500),
        _ if phase == MotionPhase::Reaction => (105, 420),
        _ => (135, 400),
    };
    Some(ChatPerformanceCue {
        intent: intent.to_string(),
        at_ms: 0,
        intensity: (0.55 + energy * 0.45).clamp(0.5, 1.4),
        tempo: (0.8 + energy * 0.3).clamp(0.6, 1.4),
        fade_in_ms,
        fade_out_ms,
        // The floor never stomps acting that is already richer than it.
        interrupt: "if-lower".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_merope::{
        PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS, PERFORMANCE_INTERRUPT_MODES,
        PERFORMANCE_POSTURES, plan_is_empty,
    };

    fn mood(band: &str, delta: f64) -> MoodTransition {
        MoodTransition {
            before: 50.0,
            after: 50.0 + delta,
            arousal_before: 48.0,
            arousal_after: 48.0,
            band_before: "calm".to_string(),
            band_after: band.to_string(),
            delta,
            cause: "test".to_string(),
            revision: 1,
        }
    }

    #[test]
    fn every_phase_but_mood_renders_a_playable_plan() {
        for phase in [
            MotionPhase::Reaction,
            MotionPhase::Delivery,
            MotionPhase::Outcome,
            MotionPhase::Proactive,
        ] {
            for style in ["restrained", "even", "open"] {
                for band in ["floor", "sad", "tense", "calm", "excited"] {
                    let plan = local_performance_plan(phase, &mood(band, 0.0), Some(true), style);
                    assert!(!plan_is_empty(&plan), "{phase:?}/{style}/{band} was empty");
                    assert_eq!(plan.cues.len(), 1);
                }
            }
        }
    }

    /// `mood_band` splits low mood on arousal; the floor has to keep the split.
    #[test]
    fn irritation_does_not_wear_the_same_face_as_flatness() {
        let flat = local_performance_plan(MotionPhase::Reaction, &mood("sad", 0.0), None, "even");
        let irritated =
            local_performance_plan(MotionPhase::Reaction, &mood("tense", 0.0), None, "even");
        let flat = flat.baseline.expect("baseline");
        let irritated = irritated.baseline.expect("baseline");
        assert_eq!(flat.expression, "subdued");
        assert_eq!(irritated.expression, "tense");
        // The body already carried the arousal; the face used not to.
        assert!(irritated.motion_energy > flat.motion_energy);

        // Irritation is a corner of the circumplex, not a rung: a swing inside
        // the band does not walk it up or down the valence ladder.
        for delta in [-20.0, 0.0, 20.0] {
            let plan =
                local_performance_plan(MotionPhase::Reaction, &mood("tense", delta), None, "even");
            assert_eq!(plan.baseline.unwrap().expression, "tense", "{delta}");
        }
    }

    #[test]
    fn a_mood_shift_moves_the_baseline_without_playing_a_beat() {
        let plan = local_performance_plan(MotionPhase::Mood, &mood("excited", 12.0), None, "even");
        assert!(plan.cues.is_empty());
        assert_eq!(plan.baseline.unwrap().expression, "warm");
    }

    /// A local plan that the contract would reject is worse than no floor.
    #[test]
    fn the_floor_only_emits_contract_vocabulary() {
        for phase in [
            MotionPhase::Reaction,
            MotionPhase::Delivery,
            MotionPhase::Outcome,
            MotionPhase::Proactive,
            MotionPhase::Mood,
        ] {
            for style in ["restrained", "even", "open", "nonsense"] {
                for band in [
                    "floor", "sad", "tense", "calm", "excited", "low", "high", "nonsense",
                ] {
                    for delta in [-20.0, 0.0, 20.0] {
                        let plan =
                            local_performance_plan(phase, &mood(band, delta), Some(false), style);
                        let baseline = plan.baseline.expect("floor always sets a baseline");
                        assert!(
                            PERFORMANCE_BASELINE_EXPRESSIONS
                                .contains(&baseline.expression.as_str())
                        );
                        assert!(PERFORMANCE_POSTURES.contains(&baseline.posture.as_str()));
                        assert!((0.2..=1.4).contains(&baseline.motion_energy));
                        assert!((0.0..=1.0).contains(&baseline.attention));
                        for cue in &plan.cues {
                            assert!(PERFORMANCE_CUE_INTENTS.contains(&cue.intent.as_str()));
                            assert!(PERFORMANCE_INTERRUPT_MODES.contains(&cue.interrupt.as_str()));
                            assert!(cue.intensity.is_finite() && cue.tempo.is_finite());
                        }
                    }
                }
            }
        }
    }

    /// What was said is the director's to read; the floor only acknowledges it.
    #[test]
    fn submitted_text_and_a_reply_get_an_acknowledgement_not_a_guess_at_their_meaning() {
        for phase in [MotionPhase::Reaction, MotionPhase::Delivery] {
            let plan = local_performance_plan(phase, &mood("normal", 0.0), None, "even");
            assert_eq!(plan.cues[0].intent, "respond", "{phase:?}");
        }
    }

    #[test]
    fn immediate_floor_uses_known_affect_without_waiting_for_the_director() {
        let mut praised = mood("excited", 10.0);
        praised.cause = "user_praise".to_string();
        let delighted = local_performance_plan(MotionPhase::Reaction, &praised, None, "open");
        assert_eq!(delighted.cues[0].intent, "delight");

        let mut scolded = mood("tense", -10.0);
        scolded.cause = "user_scold".to_string();
        let speechless = local_performance_plan(MotionPhase::Reaction, &scolded, None, "even");
        assert_eq!(speechless.cues[0].intent, "speechless");
    }

    #[test]
    fn a_failed_task_does_not_celebrate() {
        let failed = local_performance_plan(
            MotionPhase::Outcome,
            &mood("sad", -12.0),
            Some(false),
            "even",
        );
        assert_eq!(failed.cues[0].intent, "respond");
        assert_eq!(failed.baseline.unwrap().expression, "withdrawn");
        let done = local_performance_plan(
            MotionPhase::Outcome,
            &mood("excited", 12.0),
            Some(true),
            "even",
        );
        assert_eq!(done.cues[0].intent, "delight");
    }

    #[test]
    fn a_restrained_persona_stays_restrained_when_the_mood_is_high() {
        let plan = local_performance_plan(
            MotionPhase::Delivery,
            &mood("excited", 0.0),
            None,
            "restrained",
        );
        let baseline = plan.baseline.unwrap();
        assert_eq!(baseline.posture, "closed");
        assert!(baseline.motion_energy < 0.8);
    }
}
