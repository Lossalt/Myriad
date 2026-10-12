use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::rig_contract::{
    BODY_POSE_HOLD_MS, BODY_POSE_MAX_HOLD_MS, BODY_POSE_MAX_TRANSITION_MS,
    BODY_POSE_MIN_TRANSITION_MS, BODY_POSE_TRANSITION_MS, PERFORMANCE_ATTENTION_LEVELS,
    PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_BODY_CONTROL_RESTS,
    PERFORMANCE_BODY_CONTROL_TIERS, PERFORMANCE_BODY_CONTROLS, PERFORMANCE_CUE_INTENTS,
    PERFORMANCE_ENERGY_LEVELS, PERFORMANCE_EXTENT_TIERS, PERFORMANCE_FACE_PRESETS,
    PERFORMANCE_INTENSITY_LEVELS, PERFORMANCE_INTERRUPT_MODES, PERFORMANCE_POSTURES,
    PERFORMANCE_TEMPO_LEVELS,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPerformancePlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<ChatPerformanceBaseline>,
    #[serde(default)]
    pub cues: Vec<ChatPerformanceCue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPerformanceBaseline {
    pub expression: String,
    pub posture: String,
    #[serde(deserialize_with = "energy_level")]
    pub motion_energy: f32,
    #[serde(deserialize_with = "attention_level")]
    pub attention: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<BodyPose>,
    /// The contract's look her face holds (`facePresets`), when one was
    /// chosen whole: its targets are in `pose`, and the body plays the
    /// look's own life over them, again and again while it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
}

/// Normalized semantic goals, not renderer drivers or per-frame commands.
/// Each restatement replaces the targets; an empty target map releases them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BodyPose {
    #[serde(deserialize_with = "named_or_valued_targets")]
    pub targets: BTreeMap<String, f32>,
    pub transition_ms: u32,
    /// Zero holds until revised/released. Nonzero expires locally.
    pub hold_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawBodyPose {
    targets: BTreeMap<String, RawExtent>,
    #[serde(default = "default_pose_transition")]
    transition_ms: u32,
    #[serde(default = "default_pose_hold")]
    hold_ms: u32,
}

/// How far a control goes: a tier the director names, or a value a person
/// (workbench, stored standing acting) wrote.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawExtent {
    Value(f32),
    Tier(String),
}

/// The tiers the director may name for a control: toward each end it can move
/// from its rest, `-` toward its minimum. An eye open at rest only closes; a
/// drawing swapped in whole has only its full tier.
pub fn extent_tiers(control: &str) -> Vec<String> {
    let Some((_, min, max, _, _)) = PERFORMANCE_BODY_CONTROLS.iter().find(|c| c.0 == control)
    else {
        return Vec::new();
    };
    let rest = control_rest(control);
    let tiers = control_tiers(control);
    let mut named = Vec::new();
    if *min < rest {
        named.extend(tiers.iter().rev().map(|(tier, _)| format!("-{tier}")));
    }
    if *max > rest {
        named.extend(tiers.iter().map(|(tier, _)| (*tier).to_owned()));
    }
    named
}

/// A named tier on a control as its normalized value: that share of the way
/// from rest to the end the sign points at. None for a tier it does not have.
pub fn tier_extent(control: &str, tier: &str) -> Option<f32> {
    let (_, min, max, _, _) = PERFORMANCE_BODY_CONTROLS.iter().find(|c| c.0 == control)?;
    let (toward_min, name) = match tier.strip_prefix('-') {
        Some(name) => (true, name),
        None => (false, tier),
    };
    let (_, share) = control_tiers(control).iter().find(|(t, _)| *t == name)?;
    let rest = control_rest(control);
    let end = if toward_min { *min } else { *max };
    (end != rest).then(|| rest + share * (end - rest))
}

/// A control's own tiers where its look does not grow evenly, else the shared ones.
fn control_tiers(control: &str) -> &'static [(&'static str, f32)] {
    PERFORMANCE_BODY_CONTROL_TIERS
        .iter()
        .find(|(name, _)| *name == control)
        .map_or(PERFORMANCE_EXTENT_TIERS, |(_, tiers)| tiers)
}

/// Targets as written, a named tier read as its extent; unbounded values stay
/// as they are, so a bound applied later still shows.
fn named_or_valued_targets<'de, D>(deserializer: D) -> Result<BTreeMap<String, f32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    BTreeMap::<String, RawExtent>::deserialize(deserializer)?
        .into_iter()
        .map(|(control, extent)| {
            let value = match extent {
                RawExtent::Value(value) => Some(value),
                RawExtent::Tier(tier) => tier_extent(&control, &tier),
            };
            value
                .map(|value| (control, value))
                .ok_or_else(|| serde::de::Error::custom("unknown extent tier"))
        })
        .collect()
}

fn control_rest(control: &str) -> f32 {
    PERFORMANCE_BODY_CONTROL_RESTS
        .iter()
        .find(|(name, _)| *name == control)
        .map_or(0.0, |(_, rest)| *rest)
}

fn default_pose_transition() -> u32 {
    BODY_POSE_TRANSITION_MS
}
fn default_pose_hold() -> u32 {
    BODY_POSE_HOLD_MS
}

/// A pose as the director wrote it, bounded; None for an unknown or unbounded control.
pub(crate) fn sanitize_body_pose(value: &serde_json::Value) -> Option<BodyPose> {
    serde_json::from_value::<RawBodyPose>(value.clone())
        .ok()
        .and_then(sanitize_pose)
}

fn sanitize_pose(raw: RawBodyPose) -> Option<BodyPose> {
    // Reject misspelled or unbounded axes rather than silently execute half a pose.
    let mut targets = BTreeMap::new();
    for (key, extent) in raw.targets {
        let (_, min, max, _, _) = PERFORMANCE_BODY_CONTROLS.iter().find(|c| c.0 == key)?;
        let value = match extent {
            RawExtent::Value(value) if value.is_finite() => value,
            RawExtent::Value(_) => return None,
            RawExtent::Tier(tier) => tier_extent(&key, &tier)?,
        };
        targets.insert(key, value.clamp(*min, *max));
    }
    Some(BodyPose {
        targets,
        transition_ms: raw
            .transition_ms
            .clamp(BODY_POSE_MIN_TRANSITION_MS, BODY_POSE_MAX_TRANSITION_MS),
        hold_ms: raw.hold_ms.min(BODY_POSE_MAX_HOLD_MS),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPerformanceCue {
    pub intent: String,
    pub at_ms: u32,
    #[serde(deserialize_with = "intensity_level")]
    pub intensity: f32,
    #[serde(deserialize_with = "tempo_level")]
    pub tempo: f32,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
    pub interrupt: String,
}

#[derive(Debug, Deserialize)]
struct RawPlan {
    #[serde(default)]
    baseline: Option<RawBaseline>,
    #[serde(default)]
    cues: Vec<RawCue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBaseline {
    expression: String,
    posture: String,
    #[serde(default = "default_motion_energy", deserialize_with = "energy_level")]
    motion_energy: f32,
    #[serde(default = "default_attention", deserialize_with = "attention_level")]
    attention: f32,
    #[serde(default)]
    pose: Option<RawBodyPose>,
    #[serde(default)]
    face: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCue {
    intent: String,
    #[serde(default)]
    at_ms: u32,
    #[serde(default = "default_intensity", deserialize_with = "intensity_level")]
    intensity: f32,
    #[serde(default = "default_tempo", deserialize_with = "tempo_level")]
    tempo: f32,
    #[serde(default = "default_fade_in")]
    fade_in_ms: u32,
    #[serde(default = "default_fade_out")]
    fade_out_ms: u32,
    #[serde(default = "default_interrupt")]
    interrupt: String,
}

/// Parses the Lite motion director's plan. Semantic acting is selected here and
/// nowhere else: a reply model never gets to author its own performance.
pub fn parse_performance_plan(raw: &str) -> Option<ChatPerformancePlan> {
    serde_json::from_str::<RawPlan>(strip_json_fence(raw.trim()))
        .ok()
        .and_then(sanitize_plan)
}

fn strip_json_fence(value: &str) -> &str {
    value
        .strip_prefix("```json")
        .or_else(|| value.strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .unwrap_or(value)
        .trim()
}

fn sanitize_plan(plan: RawPlan) -> Option<ChatPerformancePlan> {
    let baseline = plan.baseline.and_then(sanitize_baseline);
    let cues = plan
        .cues
        .into_iter()
        .take(3)
        .filter_map(sanitize_cue)
        .collect::<Vec<_>>();
    if baseline.is_none() && cues.is_empty() {
        return None;
    }
    Some(ChatPerformancePlan { baseline, cues })
}

fn sanitize_baseline(baseline: RawBaseline) -> Option<ChatPerformanceBaseline> {
    let pose = match baseline.pose {
        Some(raw) => Some(sanitize_pose(raw)?),
        None => None,
    };
    if !PERFORMANCE_BASELINE_EXPRESSIONS.contains(&baseline.expression.as_str())
        || !PERFORMANCE_POSTURES.contains(&baseline.posture.as_str())
        || !baseline.motion_energy.is_finite()
        || !baseline.attention.is_finite()
    {
        return None;
    }
    Some(ChatPerformanceBaseline {
        expression: baseline.expression,
        posture: baseline.posture,
        motion_energy: baseline.motion_energy.clamp(0.2, 1.4),
        attention: baseline.attention.clamp(0.0, 1.0),
        pose,
        // A look the contract does not know is no look at all.
        face: baseline.face.filter(|face| {
            PERFORMANCE_FACE_PRESETS
                .iter()
                .any(|(name, _, _)| name == face)
        }),
    })
}

fn sanitize_cue(cue: RawCue) -> Option<ChatPerformanceCue> {
    if !PERFORMANCE_CUE_INTENTS.contains(&cue.intent.as_str())
        || !cue.intensity.is_finite()
        || !cue.tempo.is_finite()
    {
        return None;
    }
    Some(ChatPerformanceCue {
        intent: cue.intent,
        at_ms: cue.at_ms.min(5_000),
        intensity: cue.intensity.clamp(0.2, 1.4),
        tempo: cue.tempo.clamp(0.5, 1.6),
        fade_in_ms: cue.fade_in_ms.clamp(40, 600),
        fade_out_ms: cue.fade_out_ms.clamp(60, 800),
        interrupt: if PERFORMANCE_INTERRUPT_MODES.contains(&cue.interrupt.as_str()) {
            cue.interrupt
        } else {
            default_interrupt()
        },
    })
}

/// A number as written, or a named level the director picked (`levels` in
/// the performance contract). Bounds are applied later, so they still show.
fn level<'de, D>(deserializer: D, levels: &[(&str, f32)]) -> Result<f32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match RawExtent::deserialize(deserializer)? {
        RawExtent::Value(value) => Ok(value),
        RawExtent::Tier(name) => level_value(levels, &name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown level {name}"))),
    }
}

/// A named level's value (`levels` in the performance contract).
pub fn level_value(levels: &[(&str, f32)], name: &str) -> Option<f32> {
    levels
        .iter()
        .find(|(level, _)| *level == name)
        .map(|(_, value)| *value)
}

fn energy_level<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    level(deserializer, PERFORMANCE_ENERGY_LEVELS)
}

fn attention_level<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    level(deserializer, PERFORMANCE_ATTENTION_LEVELS)
}

fn intensity_level<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    level(deserializer, PERFORMANCE_INTENSITY_LEVELS)
}

fn tempo_level<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    level(deserializer, PERFORMANCE_TEMPO_LEVELS)
}

fn default_intensity() -> f32 {
    1.0
}

fn default_tempo() -> f32 {
    1.0
}

fn default_fade_in() -> u32 {
    150
}

fn default_fade_out() -> u32 {
    220
}

fn default_interrupt() -> String {
    "if-lower".to_string()
}

fn default_motion_energy() -> f32 {
    1.0
}

fn default_attention() -> f32 {
    0.8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_pose_is_a_bounded_full_restatement_and_empty_targets_release() {
        let plan = parse_performance_plan(r#"{"baseline":{"expression":"steady","posture":"neutral","pose":{"targets":{"headTurn":9,"torsoRise":-9,"eyeOpenLeft":-1},"transitionMs":0,"holdMs":99999}}}"#).unwrap();
        let pose = plan.baseline.unwrap().pose.unwrap();
        assert_eq!(pose.targets["headTurn"], 1.0);
        assert_eq!(pose.targets["torsoRise"], -1.0);
        assert_eq!(pose.targets["eyeOpenLeft"], 0.0);
        assert_eq!(pose.transition_ms, BODY_POSE_MIN_TRANSITION_MS);
        assert_eq!(pose.hold_ms, BODY_POSE_MAX_HOLD_MS);
        let release = parse_performance_plan(r#"{"baseline":{"expression":"steady","posture":"neutral","pose":{"targets":{},"holdMs":0}}}"#).unwrap();
        let pose = release.baseline.unwrap().pose.unwrap();
        assert!(pose.targets.is_empty());
        assert_eq!(pose.hold_ms, 0);
        assert!(parse_performance_plan(r#"{"baseline":{"expression":"steady","posture":"neutral","pose":{"targets":{"angleX":1}}}}"#).is_none());
    }

    #[test]
    fn lite_plan_parser_bounds_out_of_range_cues() {
        let plan = parse_performance_plan(
            r#"{"cues":[{"intent":"delight","atMs":9000,"intensity":9,"tempo":0.1,"interrupt":"unsafe"}]}"#,
        )
        .unwrap();
        let cue = &plan.cues[0];
        assert_eq!(cue.intent, "delight");
        assert_eq!(cue.at_ms, 5_000);
        assert_eq!(cue.intensity, 1.4);
        assert_eq!(cue.tempo, 0.5);
        assert_eq!(cue.interrupt, "if-lower");
    }

    #[test]
    fn parses_and_bounds_lite_selected_baseline() {
        let plan = parse_performance_plan(
            r#"{"baseline":{"expression":"warm","posture":"open","motionEnergy":9,"attention":-2},"cues":[]}"#,
        )
        .unwrap();
        let baseline = plan.baseline.unwrap();
        assert_eq!(baseline.expression, "warm");
        assert_eq!(baseline.posture, "open");
        assert_eq!(baseline.motion_energy, 1.4);
        assert_eq!(baseline.attention, 0.0);
    }

    #[test]
    fn accepts_think_dizzy_and_cry_as_bounded_semantic_cues() {
        let plan = parse_performance_plan(
            r#"{"cues":[{"intent":"think","atMs":0,"intensity":0.9,"tempo":0.8,"fadeInMs":160,"fadeOutMs":320,"interrupt":"queue"},{"intent":"dizzy","atMs":120,"intensity":1.2,"tempo":0.8,"fadeInMs":180,"fadeOutMs":420,"interrupt":"if-lower"},{"intent":"cry","atMs":180,"intensity":1.4,"tempo":0.7,"fadeInMs":260,"fadeOutMs":500,"interrupt":"replace"}]}"#,
        )
        .unwrap();
        assert_eq!(plan.cues.len(), 3);
        assert_eq!(plan.cues[0].intent, "think");
        assert_eq!(plan.cues[1].intent, "dizzy");
        assert_eq!(plan.cues[1].at_ms, 120);
        assert_eq!(plan.cues[2].intent, "cry");
        assert_eq!(plan.cues[2].fade_out_ms, 500);
    }

    #[test]
    fn accepts_stylized_semantic_cues() {
        let plan = parse_performance_plan(
            r#"{"cues":[{"intent":"angry","intensity":1.1},{"intent":"speechless","intensity":0.8},{"intent":"silly","intensity":1.0}]}"#,
        )
        .unwrap();
        assert_eq!(plan.cues.len(), 3);
        assert_eq!(plan.cues[0].intent, "angry");
        assert_eq!(plan.cues[1].intent, "speechless");
        assert_eq!(plan.cues[2].intent, "silly");
    }

    #[test]
    fn accepts_lovestruck_as_a_semantic_cue() {
        let plan = parse_performance_plan(r#"{"cues":[{"intent":"lovestruck"}]}"#).unwrap();
        assert_eq!(plan.cues[0].intent, "lovestruck");
    }

    #[test]
    fn discards_plans_whose_baseline_is_out_of_vocabulary() {
        assert!(
            parse_performance_plan(
                r#"{"baseline":{"expression":"angry","posture":"attack"},"cues":[]}"#,
            )
            .is_none()
        );
    }

    #[test]
    fn empty_object_is_not_a_plan() {
        assert!(parse_performance_plan("{}").is_none());
        assert!(parse_performance_plan(r#"{"cues":[]}"#).is_none());
    }

    #[test]
    fn continue_is_not_a_public_plan() {
        assert!(parse_performance_plan(r#"{"continue":true}"#).is_none());
    }

    #[test]
    fn mouth_open_is_not_a_cue_intent() {
        assert!(parse_performance_plan(r#"{"cues":[{"intent":"mouth-open"}]}"#).is_none());
    }
}
