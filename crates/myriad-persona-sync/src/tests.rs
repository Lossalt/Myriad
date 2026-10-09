//! The golden files in `fixtures/`: every kind of entity, a whole envelope,
//! one comparison and one rig package, each in its exact bytes. Another
//! platform runs the same files through the shared core and must get the
//! same bytes back.
//!
//! `MYRIAD_SYNC_BLESS=1 cargo test -p myriad-persona-sync` rewrites them
//! after a deliberate change of the format.

use std::{collections::HashMap, path::PathBuf};

use myriad_merope::{
    CharacterAssetProfile, RIG_IR_VERSION, RIG_SCHEMA_VERSION, RigBone, RigManifest, RigPart,
    RigPoint, RigQuality, RigTexture, RigVertex,
};
use serde_json::{Value, json};

use super::*;
use crate::canonical::canonical_bytes;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn blessing() -> bool {
    std::env::var_os("MYRIAD_SYNC_BLESS").is_some()
}

/// Checks `bytes` against the golden file (one trailing newline), or
/// rewrites it when blessing.
fn golden(path: &str, bytes: &[u8]) {
    let path = fixtures().join(path);
    if blessing() {
        let mut out = bytes.to_vec();
        out.push(b'\n');
        std::fs::write(&path, out).unwrap();
        return;
    }
    let stored = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let stored = stored.strip_suffix(b"\n").unwrap_or(&stored);
    assert_eq!(
        String::from_utf8_lossy(stored),
        String::from_utf8_lossy(bytes),
        "{} differs from what the crate writes",
        path.display()
    );
}

fn entity_fixture(kind: Kind) -> EntityVersion {
    let path = fixtures().join(format!("entities/{kind}.json"));
    let raw = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_slice(&raw).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn all_entities() -> Vec<EntityVersion> {
    Kind::ALL
        .into_iter()
        .map(|kind| canonicalize(entity_fixture(kind)).unwrap())
        .collect()
}

#[test]
fn every_kind_has_a_golden_entity_that_canonicalizes_to_itself() {
    for kind in Kind::ALL {
        let canonical = canonicalize(entity_fixture(kind)).unwrap();
        assert_eq!(canonical.kind, kind);
        golden(
            &format!("entities/{kind}.json"),
            &canonical.canonical_bytes(),
        );
        // Its digest is of the canonical body, and a second pass is the same.
        assert_eq!(canonical.digest, digest(&canonical.body));
        assert_eq!(canonicalize(canonical.clone()).unwrap(), canonical);
    }
}

#[test]
fn a_whole_envelope_canonicalizes_to_itself() {
    let entities = all_entities();
    let mut assets: Vec<AssetRef> = Vec::new();
    for entity in &entities {
        collect_assets(&entity.body, &mut assets);
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    assets.dedup();
    let envelope = Envelope {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        lineage: Some("1f0c2a3b-4c5d-4e6f-8a9b-0c1d2e3f4a5b".into()),
        origin: "site".into(),
        embedding_model: Some(entity::EmbeddingModel {
            model: "perplexity/pplx-embed-v1-0.6b".into(),
            dims: 2,
        }),
        entities,
        embeddings: vec![entity::Embedding {
            memory_id: "mem_3b1f9a2c".into(),
            digest: "00112233445566778899aabbccddeeff".into(),
            // [1.0, -0.5] as f32 little-endian.
            vector: "AACAPwAAAL8=".into(),
        }],
        assets,
    };
    let envelope = entity::canonicalize_envelope(envelope).unwrap();
    let bytes = canonical_bytes(&serde_json::to_value(&envelope).unwrap());
    golden("envelope.json", &bytes);
    let read: Envelope = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(entity::canonicalize_envelope(read).unwrap(), envelope);
}

fn collect_assets(value: &Value, out: &mut Vec<AssetRef>) {
    match value {
        Value::Object(map) => {
            if let Ok(asset) = serde_json::from_value::<AssetRef>(value.clone()) {
                out.push(asset);
                return;
            }
            map.values().for_each(|value| collect_assets(value, out));
        }
        Value::Array(items) => items.iter().for_each(|value| collect_assets(value, out)),
        _ => {}
    }
}

/// The site has every fixture; this Mac has her under another name, one
/// memory the same, one changed, one of its own with the site's words under
/// another id, its own day of vitals and no stickers.
fn two_sides() -> (Vec<EntityVersion>, Vec<EntityVersion>) {
    let remote = all_entities();
    let by_kind: HashMap<Kind, EntityVersion> = remote
        .iter()
        .map(|entity| (entity.kind, entity.clone()))
        .collect();
    let mac = "dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60";
    let rev: Rev = "001760000000999-000000-dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60"
        .parse()
        .unwrap();
    let mut local = Vec::new();

    let mut persona = by_kind[&Kind::Persona].clone();
    persona.body["fields"]["name"] = json!({"value": " Mero ", "rev": rev, "origin": mac});
    persona.body["fields"]["likes"]["value"] = json!(["雨天", "猫"]);
    persona.body["fields"]
        .as_object_mut()
        .unwrap()
        .remove("visual.visualIdentity");
    persona.body["fields"]["agentNote"] =
        json!({"value": "只在 Mac 上", "rev": rev, "origin": mac});
    persona.digest.clear();
    local.push(canonicalize(persona).unwrap());

    let mut anchor = by_kind[&Kind::AffectAnchor].clone();
    anchor.body["affect"]["mood"] = json!(30.0);
    anchor.body["at"] = json!(1_760_000_000_999_000_i64);
    anchor.digest.clear();
    local.push(canonicalize(anchor).unwrap());

    // The same retired memory, and the same diary entry.
    local.push(by_kind[&Kind::Memory].clone());
    local.push(by_kind[&Kind::Diary].clone());

    // Her own memory, on both sides but changed here.
    let own = |id: &str, content: &str, importance: f64| {
        canonicalize(EntityVersion {
            kind: Kind::Memory,
            id: id.into(),
            rev: rev.clone(),
            origin: mac.into(),
            state: State::Live,
            body: json!({
                "kind": "doing", "content": content, "speaker": "agent", "source": "own",
                "audience": {"venue": "own", "members": []},
                "importance": importance, "validFrom": 1, "createdAt": 1,
            }),
            digest: String::new(),
        })
        .unwrap()
    };
    let mut remote = remote;
    remote.push(own("own_1", "下午读了《吾輩は猫である》第三段。", 0.4));
    local.push(own("own_1", "下午读了《吾輩は猫である》第三段。", 0.6));
    // The same words under two ids.
    remote.push(own("own_site", "去河边走了一圈。", 0.3));
    local.push(own("own_mac", " 去河边走了一圈。\n", 0.3));

    let mut day = by_kind[&Kind::VitalsDay].clone();
    day.id = format!("2026-10-08@{mac}");
    day.origin = mac.into();
    local.push(canonicalize(day).unwrap());

    for kind in [Kind::Outfit, Kind::SerialPast, Kind::SoupGame] {
        local.push(by_kind[&kind].clone());
    }
    (local, remote)
}

#[test]
fn a_comparison_of_two_sides_is_golden() {
    let (local, remote) = two_sides();
    let to_bytes =
        |entities: &[EntityVersion]| canonical_bytes(&serde_json::to_value(entities).unwrap());
    golden("compare/local.json", &to_bytes(&local));
    golden("compare/remote.json", &to_bytes(&remote));
    let comparison = compare(local, remote).unwrap();
    golden(
        "compare/comparison.json",
        &canonical_bytes(&serde_json::to_value(&comparison).unwrap()),
    );

    use compare::{Category, Status};
    assert_eq!(comparison.names.local.as_deref(), Some("Mero"));
    assert_eq!(comparison.names.remote.as_deref(), Some("Merope"));
    assert!(!comparison.same_name);
    assert!(comparison.needs_subject);
    let field = |name: &str| {
        comparison
            .persona
            .fields
            .iter()
            .find(|field| field.field == name)
            .unwrap()
    };
    assert_eq!(field("name").status, Status::Differ);
    assert_eq!(field("likes").status, Status::Differ);
    assert_eq!(field("likes").class, FieldClass::List);
    assert_eq!(field("summary").status, Status::Same);
    assert_eq!(field("agentNote").status, Status::OnlyLocal);
    assert_eq!(field("visual.visualIdentity").status, Status::OnlyRemote);
    let counts = |category: Category| comparison.categories[&category].counts.clone();
    assert_eq!(counts(Category::Memories).same, 1);
    let experiences = counts(Category::Experiences);
    assert_eq!(
        (
            experiences.differ,
            experiences.only_local,
            experiences.only_remote
        ),
        (1, 1, 1)
    );
    assert_eq!(comparison.memory_twins.len(), 1);
    assert_eq!(comparison.memory_twins[0].local, "own_mac");
    assert_eq!(counts(Category::Vitals).only_local, 1);
    assert_eq!(counts(Category::Vitals).only_remote, 1);
    assert_eq!(counts(Category::Stickers).only_remote, 1);
    assert_eq!(counts(Category::Serial).same, 1);
    assert_eq!(counts(Category::Serial).only_remote, 3);
    assert_eq!(counts(Category::AffectEvents).only_remote, 1);
    assert_eq!(comparison.affect.events.remote, 1);
    assert_eq!(comparison.affect.events.local, 0);
    assert_eq!(
        comparison.affect.anchor.local.as_ref().unwrap().affect.mood,
        30.0
    );
}

#[test]
fn the_same_name_needs_no_choice_and_one_side_alone_is_not_a_comparison_of_two() {
    let remote = all_entities();
    let same = compare(remote.clone(), remote.clone()).unwrap();
    assert!(same.same_name && !same.needs_subject);
    assert!(
        same.categories
            .values()
            .all(|category| category.counts.differ == 0
                && category.counts.only_local == 0
                && category.counts.only_remote == 0)
    );
    let alone = compare(Vec::new(), remote).unwrap();
    assert!(!alone.needs_subject);
    assert!(!alone.persona.present.local && alone.persona.present.remote);
}

#[test]
fn a_side_listing_an_entity_twice_is_refused() {
    let one = canonicalize(entity_fixture(Kind::Diary)).unwrap();
    assert!(compare(vec![one.clone(), one], Vec::new()).is_err());
}

#[test]
fn empty_means_no_persona_and_nothing_she_remembers_wrote_or_felt() {
    assert!(is_empty(&Holdings::default()));
    assert!(is_empty(&Holdings::of(&[canonicalize(entity_fixture(
        Kind::VitalsDay
    ))
    .unwrap()])));
    for kind in [Kind::Persona, Kind::Memory, Kind::Diary, Kind::AffectEvent] {
        let holdings = Holdings::of(&[canonicalize(entity_fixture(kind)).unwrap()]);
        assert!(!is_empty(&holdings), "{kind}");
    }
}

pub(crate) fn sample_manifest() -> RigManifest {
    let vertex = |x: f32, y: f32| RigVertex {
        position: RigPoint { x, y },
        uv: RigPoint { x, y },
        joints: [0, 0, 0, 0],
        weights: [1.0, 0.0, 0.0, 0.0],
    };
    RigManifest {
        schema_version: RIG_SCHEMA_VERSION,
        rig_ir_version: Some(RIG_IR_VERSION),
        character_asset_contract_version: Some(
            CharacterAssetProfile::Bust.contract().contract_version,
        ),
        profile: CharacterAssetProfile::Bust,
        source_master_asset_id: Some("/master-a.png".into()),
        source_generation_fingerprint: Some("a".repeat(64)),
        quality: RigQuality::Layered2d,
        canvas: myriad_merope::RigSize {
            width: 1.0,
            height: 1.3333334,
        },
        textures: vec![RigTexture {
            id: "atlas".into(),
            url: "/atlas.png".into(),
            width: 256,
            height: 256,
        }],
        bones: vec![RigBone {
            id: "root".into(),
            parent: None,
            pivot: RigPoint { x: 0.5, y: 0.8 },
        }],
        parts: vec![RigPart {
            id: "body".into(),
            texture_id: "atlas".into(),
            z_index: 0,
            opacity: 1.0,
            slot: None,
            variant: None,
            vertices: vec![vertex(0.0, 0.0), vertex(1.0, 0.0), vertex(0.0, 1.0)],
            indices: vec![0, 1, 2],
        }],
        motion_profile: None,
        outfit_profile: None,
        semantic_anchors: HashMap::new(),
        semantics: None,
        spatial_profile: None,
        anime25d_playback: None,
    }
}

/// The atlas of the golden rig: its bytes are these.
pub(crate) const SAMPLE_ATLAS: &[u8] = b"atlas-pixels";

#[test]
fn a_rig_package_id_is_golden_and_ignores_where_the_atlas_is_served() {
    let manifest = sample_manifest();
    golden(
        "rig/manifest.json",
        &canonical_bytes(&serde_json::to_value(&manifest).unwrap()),
    );
    let id = rig_package_id(SAMPLE_ATLAS, &manifest).unwrap();
    golden("rig/package-id.txt", id.as_bytes());

    let stored: RigManifest =
        serde_json::from_slice(&std::fs::read(fixtures().join("rig/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(rig_package_id(SAMPLE_ATLAS, &stored).unwrap(), id);
    let mut served = manifest.clone();
    served.textures[0].url = "/api/merope/rig/assets/runtime-id".into();
    assert_eq!(rig_package_id(SAMPLE_ATLAS, &served).unwrap(), id);
    assert_ne!(rig_package_id(b"other", &manifest).unwrap(), id);
}
