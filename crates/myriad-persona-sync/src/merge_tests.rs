use serde_json::{Value, json};

use super::*;

fn fixture(name: &str) -> Vec<EntityVersion> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("fixtures/merge/{name}.json"));
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// The plan as JSON, checked against its golden file (written anew with
/// `MYRIAD_BLESS=1`).
fn golden(name: &str, plan: &MergePlan) -> Value {
    let value = serde_json::to_value(plan).unwrap();
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("fixtures/merge/{name}.json"));
    if std::env::var_os("MYRIAD_BLESS").is_some() {
        let mut text = serde_json::to_string_pretty(&value).unwrap();
        text.push('\n');
        std::fs::write(&path, text).unwrap();
    }
    let want: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(value, want, "{name} differs from its golden file");
    value
}

fn input(other: Vec<EntityVersion>, subject: Option<Side>) -> MergeInput {
    MergeInput {
        host: fixture("host"),
        other,
        subject,
        same_lineage: false,
        rev: "001760001000000-000000-site".parse().unwrap(),
        other_end: "local".into(),
        other_end_on_host: Some("app:dev_mac1".into()),
        host_end: "site".into(),
        other_device: Some(Device {
            id: "dev_mac1".into(),
            name: Some("书房的 Mac".into()),
        }),
    }
}

fn named(mut other: Vec<EntityVersion>, name: &str) -> Vec<EntityVersion> {
    for entity in &mut other {
        if entity.kind == Kind::Persona {
            entity.body["fields"]["name"]["value"] = json!(name);
            entity.digest.clear();
        }
    }
    other
}

fn entity<'a>(plan: &'a MergePlan, kind: Kind, id: &str) -> &'a EntityVersion {
    plan.entities
        .iter()
        .find(|entity| entity.kind == kind && entity.id == id)
        .unwrap_or_else(|| panic!("{kind} {id} is not in the plan"))
}

fn has(plan: &MergePlan, kind: Kind, id: &str) -> bool {
    plan.entities
        .iter()
        .any(|entity| entity.kind == kind && entity.id == id)
}

fn field<'a>(plan: &'a MergePlan, name: &str) -> &'a Value {
    &entity(plan, Kind::Persona, "her").body["fields"][name]["value"]
}

fn histories(plan: &MergePlan) -> Vec<(String, Value, String)> {
    plan.entities
        .iter()
        .filter(|entity| entity.kind == Kind::FieldHistory)
        .filter_map(|entity| entity.body_as::<FieldHistoryBody>().ok())
        .map(|past| (past.field, past.value, past.reason))
        .collect()
}

/// Names differ and the owner chose the Mac's her: her persona is the
/// Mac's exactly, the site's values are history (`subject`), the site's
/// default set is kept as an ordinary one, both ends wear what the Mac
/// wore, her affect starts from the Mac's anchor and everything both lived
/// is there.
#[test]
fn names_differ_the_subject_is_her() {
    let plan = plan(input(fixture("other"), Some(Side::Other))).unwrap();
    golden("names_differ", &plan);
    assert!(!plan.same_name);
    assert_eq!(field(&plan, "name"), "Luna");
    assert_eq!(field(&plan, "personality"), "活泼，爱笑。");
    // What the subject left unset stays unset (H3d-4).
    assert_eq!(field(&plan, "zone"), &Value::Null);
    assert_eq!(field(&plan, "displayName"), &Value::Null);
    let past = histories(&plan);
    for (name, value) in [
        ("name", json!("Merope")),
        ("personality", json!("安静，好奇。")),
        ("zone", json!("Asia/Tokyo")),
        ("displayName", json!("Merope")),
    ] {
        assert!(
            past.contains(&(name.into(), value.clone(), "subject".into())),
            "{name} {value} is not in her history: {past:?}"
        );
    }
    // The subject's default is the default; the site's is kept under a new id.
    let default: OutfitBody = entity(&plan, Kind::Outfit, "default").body_as().unwrap();
    assert_eq!(default.clothing_style, "lounge");
    let moved = &plan.renamed[0];
    assert_eq!((moved.side, moved.from.as_str()), (Side::Host, "default"));
    let kept: OutfitBody = entity(&plan, Kind::Outfit, &moved.to).body_as().unwrap();
    assert_eq!(kept.name.as_deref(), Some("Merope的默认服装"));
    assert!(
        plan.removes.is_empty(),
        "the default is the Mac's now, not gone"
    );
    // Which set each end wears (H3d-6).
    let wearing: WearingBody = entity(&plan, Kind::Wearing, "her").body_as().unwrap();
    assert_eq!(wearing.ends["site"].outfit.as_deref(), Some("default"));
    assert_eq!(
        wearing.ends["app:dev_mac1"].outfit.as_deref(),
        Some("default")
    );
    // Her affect from the subject's anchor; the site's change after it is replayed.
    let anchor: AffectAnchorBody = entity(&plan, Kind::AffectAnchor, "owner")
        .body_as()
        .unwrap();
    assert_eq!(anchor.affect.mood, 60.0);
    assert!(plan.late.is_empty());
    assert!(
        plan.dropped
            .iter()
            .any(|gone| gone.kind == Kind::AffectAnchor && gone.side == Side::Host)
    );
    assert!(has(&plan, Kind::AffectEvent, "e_s1") && has(&plan, Kind::AffectEvent, "e_m2"));
    // Both lived: memories, conversations, lines, her proposal for that Mac.
    for (kind, id) in [
        (Kind::Memory, "m_site"),
        (Kind::Memory, "m_mac"),
        (Kind::Conversation, "ses_a"),
        (Kind::Conversation, "ses_m"),
        (Kind::ProactiveLine, "nat_1"),
        (Kind::VitalsDay, "2026-10-08@site"),
        (Kind::VitalsDay, "2026-10-07@local"),
    ] {
        assert!(has(&plan, kind, id), "{kind} {id}");
    }
    // The site's day stays; the Mac's same day does not come (H3d-9).
    assert!(!has(&plan, Kind::VitalsDay, "2026-10-08@local"));
    let proposal = &entity(&plan, Kind::Intention, "int_mac").body["proposal"];
    assert_eq!(proposal["place"], "mac");
    assert_eq!(proposal["device_id"], "dev_mac1");
    // Do not disturb as the site keeps it; the later times (H3d-8).
    let state: OwnerStateBody = entity(&plan, Kind::OwnerState, "owner").body_as().unwrap();
    assert!(state.do_not_disturb);
    assert_eq!(state.dnd_start_minute, Some(1380));
    assert_eq!(state.activity.as_deref(), Some("在看书"));
    assert_eq!(
        state.last_user_message_at,
        Some(1_760_000_000_000_000 + 2 * 3_600_000_000)
    );
    let pace: PaceBody = entity(&plan, Kind::Pace, "pace").body_as().unwrap();
    assert_eq!(pace.lazed["2026-10-08"], 10.0);
    assert_eq!(pace.lazed["2026-10-07"], 5.0);
    assert!(plan.standing.is_some());
}

/// The same name: one her. A single field takes the later edit and keeps
/// the other (`lww`), a list keeps both sides' items, a field one side
/// leaves unset takes the other's value, the site's default set stays the
/// default (H3d-5), the later anchor is where replay starts and the Mac's
/// change from before it is applied right after it, marked late.
#[test]
fn the_same_name_is_one_her() {
    let plan = plan(input(named(fixture("other"), "Merope"), None)).unwrap();
    golden("names_same", &plan);
    assert!(plan.same_name);
    assert_eq!(plan.subject, None);
    assert_eq!(field(&plan, "personality"), "活泼，爱笑。");
    assert_eq!(field(&plan, "likes"), &json!(["雨天", "旧书"]));
    assert_eq!(field(&plan, "zone"), "Asia/Tokyo");
    assert_eq!(field(&plan, "displayName"), "Merope");
    let past = histories(&plan);
    assert!(past.contains(&("personality".into(), json!("安静，好奇。"), "lww".into())));
    assert!(
        !past
            .iter()
            .any(|(name, _, _)| name == "likes" || name == "zone")
    );
    let default: OutfitBody = entity(&plan, Kind::Outfit, "default").body_as().unwrap();
    assert_eq!(default.clothing_style, "everyday");
    let moved = &plan.renamed[0];
    assert_eq!((moved.side, moved.from.as_str()), (Side::Other, "default"));
    let theirs: OutfitBody = entity(&plan, Kind::Outfit, &moved.to).body_as().unwrap();
    assert_eq!(theirs.name.as_deref(), Some("另一边的默认服装"));
    let wearing: WearingBody = entity(&plan, Kind::Wearing, "her").body_as().unwrap();
    assert_eq!(wearing.ends["site"].outfit.as_deref(), Some("coat"));
    assert_eq!(
        wearing.ends["app:dev_mac1"].outfit.as_deref(),
        Some(moved.to.as_str())
    );
    // The site's anchor is later; the Mac's older change comes right after it.
    let anchor: AffectAnchorBody = entity(&plan, Kind::AffectAnchor, "owner")
        .body_as()
        .unwrap();
    assert_eq!(anchor.at, 1_760_000_000_000_000);
    assert_eq!(plan.late, vec!["e_m1".to_string()]);
    let stamped: AffectEventBody = entity(&plan, Kind::AffectEvent, "e_m1").body_as().unwrap();
    assert!(stamped.late);
    assert_eq!(stamped.at, anchor.at + 1);
    assert!(
        plan.writes
            .iter()
            .any(|write| write.kind == Kind::AffectEvent && write.id == "e_m1")
    );
}

/// A `default` on both sides: whichever stays the default, the other is
/// kept, never lost, and a set drawn from it follows it to its new id.
#[test]
fn a_default_on_both_sides_is_kept_as_another_set() {
    let mut other = fixture("other");
    let mut full = other
        .iter()
        .find(|entity| entity.kind == Kind::Outfit && entity.id == "mac-hat")
        .unwrap()
        .clone();
    full.id = "mac-full".into();
    full.body["profile"] = json!("fullBody");
    full.body["referenceOutfitId"] = json!("default");
    full.digest.clear();
    other.push(full);
    for subject in [Some(Side::Host), Some(Side::Other)] {
        let plan = plan(input(other.clone(), subject)).unwrap();
        let outfits = plan
            .entities
            .iter()
            .filter(|entity| entity.kind == Kind::Outfit)
            .count();
        assert_eq!(
            outfits, 5,
            "{subject:?}: two defaults, coat, hat and the full set"
        );
        let reference = &entity(&plan, Kind::Outfit, "mac-full").body["referenceOutfitId"];
        let expected = if subject == Some(Side::Other) {
            json!("default")
        } else {
            json!(plan.renamed[0].to)
        };
        assert_eq!(reference, &expected, "{subject:?}");
    }
}

/// Changes older than the anchor are applied right after it in the order
/// they happened (not by id), marked late; replay counts them.
#[test]
fn late_changes_keep_their_order_after_the_anchor() {
    let mut other = named(fixture("other"), "Merope");
    let mut earlier = other
        .iter()
        .find(|entity| entity.id == "e_m1")
        .unwrap()
        .clone();
    earlier.id = "zzz".into();
    earlier.body["at"] = json!(1_760_000_000_000_000_i64 - 5 * 3_600_000_000);
    earlier.digest.clear();
    other.push(earlier);
    let plan = plan(input(other.clone(), None)).unwrap();
    let at = |id: &str| -> i64 {
        entity(&plan, Kind::AffectEvent, id)
            .body_as::<AffectEventBody>()
            .unwrap()
            .at
    };
    assert_eq!(plan.late, vec!["e_m1".to_string(), "zzz".to_string()]);
    assert!(
        at("zzz") < at("e_m1"),
        "the earlier change first, whatever its id"
    );
    assert_eq!(at("zzz"), 1_760_000_000_000_000 + 1);
    // Without the late ones she stands elsewhere: they were applied.
    let without: Vec<EntityVersion> = other
        .into_iter()
        .filter(|entity| entity.id != "e_m1" && entity.id != "zzz")
        .collect();
    let alone = super::plan(input(without, None)).unwrap();
    assert_ne!(alone.standing, plan.standing);
}

/// Two copies of one birth: an entity under one id is one thing, the later
/// edit wins, and a conversation's lines come together once.
#[test]
fn one_birth_apart_comes_together_by_id() {
    let host = fixture("host");
    let mut other = named(fixture("other"), "Merope");
    let mut coat = host
        .iter()
        .find(|entity| entity.kind == Kind::Outfit && entity.id == "coat")
        .unwrap()
        .clone();
    coat.body["name"] = json!("新大衣");
    coat.rev = "001760000600000-000000-dev_mac1".parse().unwrap();
    coat.digest.clear();
    other.push(coat);
    let mut chat = host
        .iter()
        .find(|entity| entity.kind == Kind::Conversation)
        .unwrap()
        .clone();
    // Numbered anew on the other side, and one more line.
    let mut lines = chat.body["lines"].as_array().unwrap().clone();
    for (index, line) in lines.iter_mut().enumerate() {
        line["id"] = json!(format!("n{index}"));
    }
    lines.push(
        json!({"id": "n9", "role": "user", "content": "回来了", "at": 1_760_000_900_000_000_i64}),
    );
    chat.body["lines"] = Value::Array(lines);
    chat.rev = "001760000900000-000000-dev_mac1".parse().unwrap();
    chat.digest.clear();
    other.push(chat);
    let mut merging = input(other, None);
    merging.same_lineage = true;
    let plan = plan(merging).unwrap();
    let coat: OutfitBody = entity(&plan, Kind::Outfit, "coat").body_as().unwrap();
    assert_eq!(coat.name.as_deref(), Some("新大衣"));
    assert!(!plan.renamed.iter().any(|moved| moved.from == "coat"));
    let chat: ConversationBody = entity(&plan, Kind::Conversation, "ses_a")
        .body_as()
        .unwrap();
    assert_eq!(chat.lines.len(), 3);
    assert_eq!(chat.lines[2].content, "回来了");
}

#[test]
fn names_that_differ_need_a_subject() {
    assert!(plan(input(fixture("other"), None)).is_err());
}
