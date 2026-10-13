//! What this face can do (the cues, body controls and moves the director may
//! name), and whether a plan holds up as directed.

use super::*;

pub(super) fn offered_body_controls(state: Option<&RigStateSummary>) -> Vec<&'static str> {
    myriad_merope::PERFORMANCE_BODY_CONTROLS
        .iter()
        .filter(|c| {
            state.map_or(c.3 == "head-body", |state| {
                state.capabilities.iter().any(|cap| cap == c.3)
                    && !(c.0 == "maniac" && (state.speaking || state.singing))
            })
        })
        .map(|c| c.0)
        .collect()
}

/// The moves this body can play: any one of a move's controls is enough.
pub(super) fn offered_score_moves(state: Option<&RigStateSummary>) -> Vec<&'static str> {
    let controls = offered_body_controls(state);
    myriad_merope::PERFORMANCE_SCORE_MOVES
        .iter()
        .filter(|(_, moved, _)| moved.iter().any(|control| controls.contains(control)))
        .map(|(name, _, _)| *name)
        .collect()
}

#[cfg(test)]
pub(in crate::services::agent) fn semantic_valid(raw: &str, response: &str) -> bool {
    let Some(decision) = parse_motion_decision(raw) else {
        return false;
    };
    if decision == MotionDecision::Continue {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(strip_motion_json(raw)) else {
        return false;
    };
    let Some(MotionDecision::Perform(plan)) = parse_motion_decision(raw) else {
        return false;
    };
    let Ok(unfiltered) = serde_json::from_value::<ChatPerformancePlan>(value.clone()) else {
        return false;
    };
    let phrases = grounded_speech_phrases(&value["phrases"], Some(response));
    plan == unfiltered
        && plan.cues.len() <= 2
        && phrases.len() == value["phrases"].as_array().map_or(0, Vec::len)
        && (!plan_is_empty(&plan) || !phrases.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_director_names_tiers_and_the_parser_turns_them_into_extents() {
        // Drawn eyes swapped in show ghosted halfway: only the full one is offered.
        assert_eq!(myriad_merope::extent_tiers("silly"), ["strong"]);
        assert_eq!(myriad_merope::tier_extent("cry", "slight"), Some(0.2));
        let raw = r#"{"baseline":{"expression":"steady","posture":"neutral","motionEnergy":"high","attention":"intent","pose":{"targets":{"headTilt":"-clear","eyeOpenLeft":"-strong","hairSway":"slight"},"transitionMs":250,"holdMs":0}},"cues":[{"intent":"respond","atMs":0,"intensity":"strong","tempo":"quick","fadeInMs":100,"fadeOutMs":300,"interrupt":"if-lower"}],"score":[{"text":"嗯","move":{"kind":"nod","amount":"strong","tempo":"slow"}}]}"#;
        assert!(semantic_valid(raw, "嗯，好。"));
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(raw) else {
            panic!("tiers must survive the real director parser")
        };
        let baseline = plan.baseline.unwrap();
        assert_eq!((baseline.motion_energy, baseline.attention), (1.3, 1.0));
        assert_eq!((plan.cues[0].intensity, plan.cues[0].tempo), (1.3, 1.3));
        let targets = baseline.pose.unwrap().targets;
        assert_eq!(targets["headTilt"], -0.65);
        assert_eq!(targets["eyeOpenLeft"], 0.0);
        assert_eq!(targets["hairSway"], 0.35);
        let unknown = raw.replace("\"-clear\"", "\"huge\"");
        assert!(
            !semantic_valid(&unknown, "嗯，好。"),
            "an unknown tier is not half acted"
        );
    }

    #[test]
    fn a_body_pose_round_trips_the_actual_director_parser_and_semantic_validator() {
        let raw = r#"{"baseline":{"expression":"steady","posture":"neutral","motionEnergy":1.3,"attention":1,"pose":{"targets":{"headTurn":-0.8,"torsoTurn":0.7,"torsoRise":0.6,"gazeHorizontal":0,"leftArmRaise":0.9,"rightArmRaise":0},"transitionMs":250,"holdMs":0}},"cues":[]}"#;
        assert!(semantic_valid(raw, ""));
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(raw) else {
            panic!("pose must survive the real director parser")
        };
        assert_eq!(plan.baseline.unwrap().pose.unwrap().targets.len(), 6);
    }
}
