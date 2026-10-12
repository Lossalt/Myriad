//! The way from the look she holds to the one she has just taken, asked on
//! its own once that one is known. Put in the same round as the choice of
//! face, a decisions model names the new face again as the look on the way
//! (both models, 10-12); asked of the fixed change, it keeps most changes
//! straight and passes dozing to delight through waking up surprised.

use myriad_merope::{MAX_FACES_PASSED, PLAIN_FACE, RigStateSummary};
use serde_json::{Map, Value, json};

use super::control::{look_means, offered_faces, sure};
use super::{MotionContext, MotionPhase};

/// The change of look the answers make, from the one she holds: each a
/// preset's name, or her ordinary face. None when the look stays.
pub(super) fn change(answers: &Value, state: Option<&RigStateSummary>) -> Option<(String, String)> {
    let from = state
        .and_then(|state| state.face.clone())
        .unwrap_or_else(|| PLAIN_FACE.into());
    let to = sure(answers, "face")
        .filter(|face| *face != "none")
        .unwrap_or(PLAIN_FACE)
        .to_owned();
    (from != to).then_some((from, to))
}

/// The two questions of the way there: a look passed through first, and
/// rarely a second; her ordinary face is one of the looks.
pub(super) fn questions(
    from: &str,
    to: &str,
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> (Value, Value) {
    let mut looks = Map::new();
    looks.insert(
        "none".into(),
        json!("nothing (more) in between: straight on"),
    );
    if from != PLAIN_FACE && to != PLAIN_FACE {
        looks.insert(
            PLAIN_FACE.into(),
            json!("her ordinary face: calming down, collecting herself"),
        );
    }
    for (name, means) in offered_faces(state) {
        if name != from && name != to {
            looks.insert(name.into(), json!(means));
        }
    }
    let moment = match (&context.phase, context.response_text.as_deref()) {
        (MotionPhase::Presence, _) => format!("While she is {}", context.activity),
        (_, Some(reply)) => format!(
            "Someone said: 「{}」. She answers: 「{reply}」",
            context.user_text
        ),
        (_, None) => format!("Someone said: 「{}」", context.user_text),
    };
    let questions = json!({
        "first": {"type": "choice",
            "instructions": format!(
                "{moment}. Her face changes from {} to {}. Does it pass through another look on the way, \
                 as real faces sometimes do: dozing to delight through waking up surprised, tears to laughter \
                 through a wet smile, sulking to shyness through calming down, joy to sorrow through a moment \
                 of shock? Most changes go straight.",
                look_means(from), look_means(to)
            ),
            "criteria": looks.clone()},
        "then": {"type": "choice",
            "instructions": format!(
                "If it passes through one, does it pass through a second after that, before {}? Rarely.",
                look_means(to)
            ),
            "criteria": looks},
    });
    let state = json!({"her face now": look_means(from), "her new face": look_means(to)});
    (state, questions)
}

/// The looks passed through, in order: the first if she is sure of one,
/// then a second only after a first.
pub(super) fn passed(answers: &Value) -> Vec<String> {
    let mut looks: Vec<String> = Vec::new();
    for key in ["first", "then"] {
        match sure(answers, key).filter(|look| *look != "none") {
            Some(look) if !looks.iter().any(|seen| seen == look) => looks.push(look.to_owned()),
            _ => break,
        }
    }
    looks.truncate(MAX_FACES_PASSED);
    looks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(face: Option<&str>) -> RigStateSummary {
        myriad_merope::sanitize_rig_state(&json!({"face": face, "capabilities":
            ["head-body", "independent-eyes", "mouth-shapes"]}))
        .unwrap()
    }

    fn answer(choice: &str, confidence: f64) -> Value {
        json!({"choice": choice, "confidence": confidence})
    }

    #[test]
    fn only_a_new_look_asks_the_way_there() {
        let drowsy = state(Some("drowsy"));
        let excited = json!({"face": answer("excited", 0.9)});
        assert_eq!(
            change(&excited, Some(&drowsy)),
            Some(("drowsy".into(), "excited".into()))
        );
        assert_eq!(
            change(&json!({"face": answer("drowsy", 0.9)}), Some(&drowsy)),
            None
        );
        // No look chosen is her ordinary face; from it to it is no change.
        assert_eq!(
            change(&json!({"face": answer("none", 0.9)}), Some(&drowsy)),
            Some(("drowsy".into(), PLAIN_FACE.into()))
        );
        assert_eq!(change(&json!({}), Some(&state(None))), None);
    }

    #[test]
    fn the_way_there_is_asked_of_this_change_without_its_own_ends() {
        let drowsy = state(Some("drowsy"));
        let mut context =
            crate::services::agent::motion_overlay::motion_refinement_tests::context();
        context.user_text = "醒醒！你中奖了！".into();
        context.response_text = Some("诶？！真的吗？！".into());
        let (asked_state, asked) = questions("drowsy", "excited", &context, Some(&drowsy));
        let first = &asked["first"];
        let instructions = first["instructions"].as_str().unwrap();
        assert!(instructions.contains("中奖") && instructions.contains("lids heavy"));
        assert!(
            first["criteria"]["surprised"].is_string() && first["criteria"]["plain"].is_string()
        );
        assert!(first["criteria"]["drowsy"].is_null() && first["criteria"]["excited"].is_null());
        assert!(
            asked_state["her new face"]
                .as_str()
                .unwrap()
                .contains("sparkles")
        );
        // To her ordinary face, that face is the end, not a look on the way.
        assert!(questions("drowsy", PLAIN_FACE, &context, Some(&drowsy)).1["first"]["criteria"]["plain"].is_null());
    }

    #[test]
    fn the_looks_passed_through_come_in_order_and_only_when_sure() {
        let both = json!({"first": answer("surprised", 0.9), "then": answer("plain", 0.6)});
        assert_eq!(
            passed(&both),
            vec!["surprised".to_owned(), PLAIN_FACE.to_owned()]
        );
        let straight = json!({"first": answer("none", 0.9), "then": answer("plain", 0.9)});
        assert!(passed(&straight).is_empty());
        let unsure = json!({"first": answer("surprised", 0.1)});
        assert!(passed(&unsure).is_empty());
        let twice = json!({"first": answer("surprised", 0.9), "then": answer("surprised", 0.9)});
        assert_eq!(passed(&twice), vec!["surprised".to_owned()]);
    }
}
