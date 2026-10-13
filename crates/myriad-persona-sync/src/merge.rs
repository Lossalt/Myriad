//! Two of her brought together the first time they meet (E7): the end she
//! will live on (the host) and another end that has a her of its own.
//!
//! - **Her names differ**: the owner says which side she is (the subject).
//!   Her persona is the subject's, field by field; what the other side had
//!   goes to her history (`subject`), a field the subject left unset stays
//!   unset (H3d-4). Everything she lived on both sides comes together.
//! - **Her names are the same**: she is one her. A single field takes the
//!   later revision and keeps the other as history (`lww`); a list keeps
//!   both sides' items.
//!
//! On both: memories, diary, stickers, vitals, serials, conversations, her
//! unprompted lines and her proposals come together (an exact twin under
//! another id is kept too). One entity on both sides under the same id is
//! the same thing when both came from one birth (`sameLineage`), and the
//! later revision wins; otherwise an outfit under an id the host has is kept
//! under a new one. Her affect starts from one anchor (the subject's, or
//! the later one when the names are the same) and replays every change of
//! both sides; a change older than that anchor is applied right after it
//! and marked `late` (H3-9), in its own order.
//!
//! Pure: the merge's own revision comes in; nothing reads a clock.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    SyncError,
    affect::{self, Change},
    compare::compared_value,
    entity::{
        AffectAnchorBody, AffectEventBody, ConversationBody, ConversationLine, EntityVersion,
        FieldHistoryBody, IntentionBody, Kind, OWNER, OutfitBody, OwnerStateBody, PERSONA_ID,
        PaceBody, ProactiveLineBody, State, WearingBody, Worn, canonicalize,
    },
    hlc::{Rev, check_node},
    persona::{FieldClass, PersonaBody, PersonaField, field_class},
};

/// One of the two sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    /// The end she will live on, which lands the merge.
    Host,
    /// The end whose her is brought in.
    Other,
}

/// The device the other side is, for what it proposed to do (H3d-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Device {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeInput {
    pub host: Vec<EntityVersion>,
    pub other: Vec<EntityVersion>,
    /// Which side she is when her names differ; not read when they are the
    /// same.
    #[serde(default)]
    pub subject: Option<Side>,
    /// Both are the same birth of her (one lineage), apart for a while.
    #[serde(default)]
    pub same_lineage: bool,
    /// The revision what the merge writes is stamped with (the host's
    /// clock; its node is the host's).
    pub rev: Rev,
    /// The other side's own end in `wearing` (`local` for an app with no
    /// site), and the name that end has on the host (`app:<device>`).
    pub other_end: String,
    #[serde(default)]
    pub other_end_on_host: Option<String>,
    /// The host's own end in `wearing` (`site`).
    pub host_end: String,
    #[serde(default)]
    pub other_device: Option<Device>,
}

/// One persona field after the merge, as the owner is shown it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldOutcome {
    pub field: String,
    /// Whose value it has: `host`, `other`, or none (a list of both, or
    /// unset).
    pub from: Option<Side>,
    /// Both sides' items (a list).
    #[serde(default)]
    pub both: bool,
    /// The side whose differing value went to her history.
    pub to_history: Option<Side>,
}

/// An entity the host writes: new to it, or in place of what it has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Write {
    pub kind: Kind,
    pub id: String,
    /// The host has an entity under this id, which this replaces.
    pub replaces: bool,
    /// Where it came from: `host` (changed by the merge, e.g. renamed),
    /// `other`, or none (made by the merge out of both).
    pub from: Option<Side>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Renamed {
    pub kind: Kind,
    pub side: Side,
    pub from: String,
    pub to: String,
}

/// Something of one side not carried into the merged her, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dropped {
    pub kind: Kind,
    pub id: String,
    pub side: Side,
    /// `anchor` (the other anchor: replay starts from one), `day` (the host
    /// has that day), `twin` (the same line already there).
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    pub host: u64,
    pub other: u64,
    pub merged: u64,
}

/// Her affect toward the owner right after the last change, as the host
/// keeps it now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    pub affect: crate::entity::Affect,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergePlan {
    pub same_name: bool,
    /// Her name on each side.
    pub names: BTreeMap<Side, Option<String>>,
    /// Who she is (none when her names are the same).
    pub subject: Option<Side>,
    /// The merged her, canonical, by kind and id.
    pub entities: Vec<EntityVersion>,
    /// What the host writes of it.
    pub writes: Vec<Write>,
    /// What the host has and no longer keeps under that id (an outfit
    /// renamed).
    pub removes: Vec<Write>,
    pub fields: Vec<FieldOutcome>,
    pub renamed: Vec<Renamed>,
    /// Changes of her affect applied right after the anchor, by id.
    pub late: Vec<String>,
    pub dropped: Vec<Dropped>,
    /// By kind.
    pub counts: BTreeMap<Kind, Count>,
    pub standing: Option<Standing>,
}

type Map = BTreeMap<(Kind, String), EntityVersion>;

/// The merged her as it is made, and which side each entity came from.
#[derive(Default)]
struct Out {
    merged: Map,
    from: BTreeMap<(Kind, String), Option<Side>>,
}

impl Out {
    fn put(&mut self, entity: EntityVersion, side: Option<Side>) {
        let key = (entity.kind, entity.id.clone());
        self.from.insert(key.clone(), side);
        self.merged.insert(key, entity);
    }
}

fn side_of(entities: Vec<EntityVersion>, which: &str) -> Result<Map, SyncError> {
    let mut out = BTreeMap::new();
    for entity in entities {
        let entity =
            canonicalize(entity).map_err(|error| SyncError::new(format!("{which}: {error}")))?;
        if out
            .insert((entity.kind, entity.id.clone()), entity)
            .is_some()
        {
            return Err(SyncError::new(format!(
                "{which}: an entity is listed twice"
            )));
        }
    }
    Ok(out)
}

fn of_kind(map: &Map, kind: Kind) -> impl Iterator<Item = &EntityVersion> {
    map.values().filter(move |entity| entity.kind == kind)
}

fn fixed(map: &Map, kind: Kind) -> Option<&EntityVersion> {
    kind.fixed_id()
        .and_then(|id| map.get(&(kind, id.to_string())))
}

fn persona(map: &Map) -> Result<Option<PersonaBody>, SyncError> {
    map.get(&(Kind::Persona, PERSONA_ID.to_string()))
        .map(EntityVersion::body_as)
        .transpose()
}

fn made<B: Serialize>(
    kind: Kind,
    id: &str,
    rev: &Rev,
    state: State,
    body: &B,
) -> Result<EntityVersion, SyncError> {
    EntityVersion::new(kind, id, rev.clone(), rev.node.clone(), state, body)
}

/// The later of two versions of one entity; the host's on a tie.
fn later<'a>(host: &'a EntityVersion, other: &'a EntityVersion) -> (&'a EntityVersion, Side) {
    if other.rev > host.rev {
        (other, Side::Other)
    } else {
        (host, Side::Host)
    }
}

fn micros(rev: &Rev) -> i64 {
    (rev.wall_ms as i64).saturating_mul(1000)
}

/// Brings the other side's her into the host's (see the module).
pub fn plan(input: MergeInput) -> Result<MergePlan, SyncError> {
    check_node(&input.rev.node)?;
    let host = side_of(input.host, "host")?;
    let other = side_of(input.other, "other")?;
    let host_persona =
        persona(&host)?.ok_or_else(|| SyncError::new("the host has no her to merge into"))?;
    let other_persona =
        persona(&other)?.ok_or_else(|| SyncError::new("the other side has no her to bring"))?;
    let names = BTreeMap::from([
        (Side::Host, host_persona.name().map(str::to_string)),
        (Side::Other, other_persona.name().map(str::to_string)),
    ]);
    // As the comparison decides it (`compare::compare`).
    let same_name = names[&Side::Host] == names[&Side::Other];
    let subject = if same_name {
        None
    } else {
        Some(
            input
                .subject
                .ok_or_else(|| SyncError::new("her names differ: the owner says who she is"))?,
        )
    };
    let rev = input.rev.clone();
    let now = micros(&rev);
    let mut out = Out::default();
    let mut renamed: Vec<Renamed> = Vec::new();
    let mut dropped: Vec<Dropped> = Vec::new();
    let mut late: Vec<String> = Vec::new();

    // Her persona, field by field.
    let (persona_body, fields, history) =
        merge_persona(&host_persona, &other_persona, subject, &rev, now)?;
    out.put(
        made(Kind::Persona, PERSONA_ID, &rev, State::Live, &persona_body)?,
        None,
    );
    let mut histories: BTreeSet<String> = BTreeSet::new();
    for entity in of_kind(&host, Kind::FieldHistory).chain(of_kind(&other, Kind::FieldHistory)) {
        if histories.insert(entity.id.clone()) {
            let side = if host.contains_key(&(entity.kind, entity.id.clone())) {
                Side::Host
            } else {
                Side::Other
            };
            out.put(entity.clone(), Some(side));
        }
    }
    for past in history {
        let id = past.id();
        if histories.insert(id.clone()) {
            out.put(
                made(Kind::FieldHistory, &id, &rev, State::Live, &past)?,
                None,
            );
        }
    }

    // Her wardrobe: whose `default` stays the default.
    let default_side = match subject {
        Some(side) => side,
        None => Side::Host,
    };
    let default_id = myriad_merope::DEFAULT_WARDROBE_ID;
    let host_default = host.contains_key(&(Kind::Outfit, default_id.to_string()));
    let other_default = other.contains_key(&(Kind::Outfit, default_id.to_string()));
    let mut outfit_ids: BTreeMap<(Side, String), String> = BTreeMap::new();
    let other_name = names[&Side::Other].clone();
    let host_name = names[&Side::Host].clone();
    let fresh_id = |entity: &EntityVersion, taken: &dyn Fn(&str) -> bool| -> String {
        let mut index = 0usize;
        loop {
            let suffix = if index == 0 {
                entity.digest[..8].to_string()
            } else {
                format!("{}{index}", &entity.digest[..8])
            };
            let id = format!("{}-{suffix}", entity.id);
            if !taken(&id) {
                return id;
            }
            index += 1;
        }
    };
    let all_outfit_ids: BTreeSet<String> = of_kind(&host, Kind::Outfit)
        .chain(of_kind(&other, Kind::Outfit))
        .map(|entity| entity.id.clone())
        .collect();
    let taken = |id: &str| all_outfit_ids.contains(id);
    // The host's default goes when the other side's is the one kept.
    if host_default && other_default && default_side == Side::Other {
        let entity = &host[&(Kind::Outfit, default_id.to_string())];
        let to = fresh_id(entity, &taken);
        outfit_ids.insert((Side::Host, default_id.to_string()), to.clone());
        renamed.push(Renamed {
            kind: Kind::Outfit,
            side: Side::Host,
            from: default_id.into(),
            to,
        });
    }
    for entity in of_kind(&other, Kind::Outfit) {
        let key = (Kind::Outfit, entity.id.clone());
        let collides = host.contains_key(&key);
        let same_thing = collides && input.same_lineage;
        let keep_default = entity.id == default_id && default_side == Side::Other;
        if collides && !same_thing && !keep_default {
            let to = fresh_id(entity, &taken);
            outfit_ids.insert((Side::Other, entity.id.clone()), to.clone());
            renamed.push(Renamed {
                kind: Kind::Outfit,
                side: Side::Other,
                from: entity.id.clone(),
                to,
            });
        }
    }
    let outfit_id = |side: Side, id: &str| -> String {
        outfit_ids
            .get(&(side, id.to_string()))
            .cloned()
            .unwrap_or_else(|| id.to_string())
    };
    let moved_outfit = |entity: &EntityVersion, side: Side| -> Result<EntityVersion, SyncError> {
        let id = outfit_id(side, &entity.id);
        if entity.state == State::Deleted {
            return Ok(EntityVersion {
                id,
                digest: String::new(),
                ..entity.clone()
            });
        }
        let mut body: OutfitBody = entity.body_as()?;
        if let Some(reference) = &body.reference_outfit_id {
            body.reference_outfit_id = Some(outfit_id(side, reference));
        }
        if entity.id == default_id && id != default_id && body.name.is_none() {
            let whose = match side {
                Side::Host => host_name.clone(),
                Side::Other => other_name.clone(),
            };
            body.name = Some(match whose {
                Some(name) if !same_name => format!("{name}的默认服装"),
                _ => "另一边的默认服装".to_string(),
            });
        }
        canonicalize(EntityVersion {
            id,
            body: serde_json::to_value(body).map_err(SyncError::from_json)?,
            digest: String::new(),
            ..entity.clone()
        })
    };
    for entity in of_kind(&host, Kind::Outfit) {
        let changed = outfit_id(Side::Host, &entity.id) != entity.id
            || entity
                .body_as::<OutfitBody>()
                .ok()
                .and_then(|body| body.reference_outfit_id)
                .is_some_and(|reference| outfit_id(Side::Host, &reference) != reference);
        out.put(
            if changed {
                moved_outfit(entity, Side::Host)?
            } else {
                entity.clone()
            },
            Some(Side::Host),
        );
    }
    for entity in of_kind(&other, Kind::Outfit) {
        let moved = moved_outfit(entity, Side::Other)?;
        let key = (Kind::Outfit, moved.id.clone());
        match out.merged.get(&key) {
            Some(kept) if kept.digest == moved.digest && kept.state == moved.state => {}
            Some(kept) => {
                let (winner, side) = later(kept, &moved);
                if side == Side::Other {
                    let winner = winner.clone();
                    out.put(winner, Some(Side::Other));
                }
            }
            None => out.put(moved, Some(Side::Other)),
        }
    }

    // Which set each end wears (H3d-6).
    let worn_of = |map: &Map| -> Result<WearingBody, SyncError> {
        Ok(fixed(map, Kind::Wearing)
            .map(EntityVersion::body_as::<WearingBody>)
            .transpose()?
            .unwrap_or_default())
    };
    let host_wearing = worn_of(&host)?;
    let other_wearing = worn_of(&other)?;
    let live_outfit = |id: &Option<String>, merged: &Map| -> Option<String> {
        id.as_ref()
            .filter(|id| {
                merged
                    .get(&(Kind::Outfit, (*id).clone()))
                    .is_some_and(|entity| entity.state == State::Live)
            })
            .cloned()
    };
    let mapped = |side: Side, worn: &Worn| Worn {
        outfit: worn.outfit.as_deref().map(|id| outfit_id(side, id)),
        full_body: worn.full_body.as_deref().map(|id| outfit_id(side, id)),
    };
    let mut ends: BTreeMap<String, Worn> = host_wearing
        .ends
        .iter()
        .map(|(end, worn)| (end.clone(), mapped(Side::Host, worn)))
        .collect();
    let other_own = other_wearing
        .ends
        .get(&input.other_end)
        .map(|worn| mapped(Side::Other, worn));
    if subject == Some(Side::Other)
        && let Some(worn) = &other_own
    {
        ends.insert(input.host_end.clone(), worn.clone());
    }
    if let (Some(end), Some(worn)) = (&input.other_end_on_host, &other_own) {
        ends.insert(end.clone(), worn.clone());
    }
    for (end, worn) in &other_wearing.ends {
        if end.starts_with("app:") && !ends.contains_key(end) {
            ends.insert(end.clone(), mapped(Side::Other, worn));
        }
    }
    let ends: BTreeMap<String, Worn> = ends
        .into_iter()
        .map(|(end, worn)| {
            let worn = Worn {
                outfit: live_outfit(&worn.outfit, &out.merged),
                full_body: live_outfit(&worn.full_body, &out.merged),
            };
            (end, worn)
        })
        .collect();
    if !ends.is_empty() || fixed(&host, Kind::Wearing).is_some() {
        let body = WearingBody { ends };
        let entity = made(Kind::Wearing, PERSONA_ID, &rev, State::Live, &body)?;
        match fixed(&host, Kind::Wearing) {
            Some(kept) if kept.digest == entity.digest => out.put(kept.clone(), Some(Side::Host)),
            _ => out.put(entity, None),
        }
    }

    // What both sides lived, together.
    for kind in [
        Kind::Memory,
        Kind::Diary,
        Kind::Sticker,
        Kind::SerialFollowing,
        Kind::SerialWork,
        Kind::Intention,
    ] {
        for entity in of_kind(&host, kind) {
            out.put(entity.clone(), Some(Side::Host));
        }
        for entity in of_kind(&other, kind) {
            let entity = if kind == Kind::Intention {
                for_this_device(entity, input.other_device.as_ref())?
            } else {
                entity.clone()
            };
            match host.get(&(kind, entity.id.clone())) {
                Some(kept) if kept.digest == entity.digest && kept.state == entity.state => {}
                Some(kept) => {
                    if later(kept, &entity).1 == Side::Other {
                        out.put(entity, Some(Side::Other));
                    }
                }
                None => out.put(entity, Some(Side::Other)),
            }
        }
    }

    // Days she counted: the host's day stays, a day only the other side has
    // comes (H3d-9).
    let day_of =
        |entity: &EntityVersion| entity.id.split('@').next().unwrap_or_default().to_string();
    let host_days: BTreeSet<String> = of_kind(&host, Kind::VitalsDay).map(day_of).collect();
    for entity in of_kind(&host, Kind::VitalsDay) {
        out.put(entity.clone(), Some(Side::Host));
    }
    for entity in of_kind(&other, Kind::VitalsDay) {
        if host_days.contains(&day_of(entity)) {
            dropped.push(Dropped {
                kind: Kind::VitalsDay,
                id: entity.id.clone(),
                side: Side::Other,
                why: "day".into(),
            });
        } else if !out
            .merged
            .contains_key(&(Kind::VitalsDay, entity.id.clone()))
        {
            out.put(entity.clone(), Some(Side::Other));
        }
    }

    // Her conversations: one under the same id is one conversation, its
    // lines together (a line already there, by who said what when, once).
    for entity in of_kind(&host, Kind::Conversation) {
        out.put(entity.clone(), Some(Side::Host));
    }
    for entity in of_kind(&other, Kind::Conversation) {
        match host.get(&(Kind::Conversation, entity.id.clone())) {
            Some(kept) if kept.digest == entity.digest => {}
            Some(kept) => {
                let ours: ConversationBody = kept.body_as()?;
                let theirs: ConversationBody = entity.body_as()?;
                let body = lines_together(ours, theirs, later(kept, entity).1);
                let newer = if entity.rev > kept.rev {
                    &entity.rev
                } else {
                    &kept.rev
                };
                let together = EntityVersion::new(
                    Kind::Conversation,
                    &entity.id,
                    newer.clone(),
                    newer.node.clone(),
                    State::Live,
                    &body,
                )?;
                if together.digest != kept.digest {
                    out.put(together, None);
                }
            }
            None => out.put(entity.clone(), Some(Side::Other)),
        }
    }
    // Her unprompted lines: the same line (when and what) once.
    let said: BTreeSet<(i64, String)> = of_kind(&host, Kind::ProactiveLine)
        .filter_map(|entity| entity.body_as::<ProactiveLineBody>().ok())
        .map(|line| (line.at, line.content))
        .collect();
    for entity in of_kind(&host, Kind::ProactiveLine) {
        out.put(entity.clone(), Some(Side::Host));
    }
    for entity in of_kind(&other, Kind::ProactiveLine) {
        let line: ProactiveLineBody = entity.body_as()?;
        if let Some(kept) = host.get(&(Kind::ProactiveLine, entity.id.clone())) {
            if kept.digest != entity.digest && later(kept, entity).1 == Side::Other {
                out.put(entity.clone(), Some(Side::Other));
            }
        } else if said.contains(&(line.at, line.content.clone())) {
            dropped.push(Dropped {
                kind: Kind::ProactiveLine,
                id: entity.id.clone(),
                side: Side::Other,
                why: "twin".into(),
            });
        } else {
            out.put(entity.clone(), Some(Side::Other));
        }
    }

    // One of a kind: the subject's, or the later one when she is one her;
    // what only one side has, from that side.
    for kind in [
        Kind::SerialPast,
        Kind::SerialShelf,
        Kind::SoupGame,
        Kind::Present,
    ] {
        match (fixed(&host, kind), fixed(&other, kind)) {
            (Some(kept), Some(theirs)) => {
                let side = match subject {
                    Some(side) => side,
                    None => later(kept, theirs).1,
                };
                match side {
                    Side::Host => out.put(kept.clone(), Some(Side::Host)),
                    Side::Other => out.put(theirs.clone(), Some(Side::Other)),
                }
            }
            (Some(kept), None) => out.put(kept.clone(), Some(Side::Host)),
            (None, Some(theirs)) => out.put(theirs.clone(), Some(Side::Other)),
            (None, None) => {}
        }
    }
    // Her pace: her usual from that one, the minutes she lazed by day from
    // both (the host's day stays, H3d-9).
    match (fixed(&host, Kind::Pace), fixed(&other, Kind::Pace)) {
        (Some(kept), Some(theirs)) => {
            let ours: PaceBody = kept.body_as()?;
            let other_pace: PaceBody = theirs.body_as()?;
            let side = match subject {
                Some(side) => side,
                None => later(kept, theirs).1,
            };
            let mut body = match side {
                Side::Host => ours.clone(),
                Side::Other => other_pace.clone(),
            };
            body.lazed = other_pace.lazed.clone();
            body.lazed.extend(ours.lazed.clone());
            let entity = made(Kind::Pace, crate::entity::PACE_ID, &rev, State::Live, &body)?;
            if entity.digest == kept.digest {
                out.put(kept.clone(), Some(Side::Host));
            } else {
                out.put(entity, None);
            }
        }
        (Some(kept), None) => out.put(kept.clone(), Some(Side::Host)),
        (None, Some(theirs)) => out.put(theirs.clone(), Some(Side::Other)),
        (None, None) => {}
    }

    // Her state toward the owner (H3d-8): when each last wrote or went, the
    // later; do not disturb as the host keeps it; what they are doing, the
    // later said.
    match (
        fixed(&host, Kind::OwnerState),
        fixed(&other, Kind::OwnerState),
    ) {
        (Some(kept), Some(theirs)) => {
            let ours: OwnerStateBody = kept.body_as()?;
            let other_state: OwnerStateBody = theirs.body_as()?;
            let latest = |a: Option<i64>, b: Option<i64>| a.max(b);
            let theirs_newer = other_state.activity.is_some()
                && other_state.activity_updated_at > ours.activity_updated_at;
            let body = OwnerStateBody {
                activity: if theirs_newer {
                    other_state.activity.clone()
                } else {
                    ours.activity.clone()
                },
                activity_updated_at: if theirs_newer {
                    other_state.activity_updated_at
                } else {
                    ours.activity_updated_at
                },
                do_not_disturb: ours.do_not_disturb,
                dnd_start_minute: ours.dnd_start_minute,
                dnd_end_minute: ours.dnd_end_minute,
                last_user_message_at: latest(
                    ours.last_user_message_at,
                    other_state.last_user_message_at,
                ),
                last_proactive_at: latest(ours.last_proactive_at, other_state.last_proactive_at),
                music_mood_credited_at: latest(
                    ours.music_mood_credited_at,
                    other_state.music_mood_credited_at,
                ),
            };
            let entity = made(Kind::OwnerState, OWNER, &rev, State::Live, &body)?;
            if entity.digest == kept.digest {
                out.put(kept.clone(), Some(Side::Host));
            } else {
                out.put(entity, None);
            }
        }
        (Some(kept), None) => out.put(kept.clone(), Some(Side::Host)),
        (None, Some(theirs)) => out.put(theirs.clone(), Some(Side::Other)),
        (None, None) => {}
    }

    // Her affect toward the owner.
    let host_anchor = fixed(&host, Kind::AffectAnchor);
    let other_anchor = fixed(&other, Kind::AffectAnchor);
    let anchor_side = match (host_anchor, other_anchor) {
        (Some(kept), Some(theirs)) => Some(match subject {
            Some(side) => side,
            None => {
                let ours: AffectAnchorBody = kept.body_as()?;
                let other_body: AffectAnchorBody = theirs.body_as()?;
                if other_body.at > ours.at {
                    Side::Other
                } else {
                    Side::Host
                }
            }
        }),
        (Some(_), None) => Some(Side::Host),
        (None, Some(_)) => Some(Side::Other),
        (None, None) => None,
    };
    let anchor = match anchor_side {
        Some(Side::Host) => host_anchor,
        Some(Side::Other) => other_anchor,
        None => None,
    };
    if let (Some(anchor), Some(side)) = (anchor, anchor_side) {
        out.put(anchor.clone(), Some(side));
        let left = match side {
            Side::Host => other_anchor,
            Side::Other => host_anchor,
        };
        if let Some(left) = left {
            dropped.push(Dropped {
                kind: Kind::AffectAnchor,
                id: left.id.clone(),
                side: if side == Side::Host {
                    Side::Other
                } else {
                    Side::Host
                },
                why: "anchor".into(),
            });
        }
    }
    let anchor_body: Option<AffectAnchorBody> = anchor.map(EntityVersion::body_as).transpose()?;
    let anchor_from = anchor_side;
    let mut events: Vec<(EntityVersion, Side)> = Vec::new();
    let mut seen_events: BTreeSet<String> = BTreeSet::new();
    for (map, side) in [(&host, Side::Host), (&other, Side::Other)] {
        for entity in of_kind(map, Kind::AffectEvent) {
            if seen_events.insert(entity.id.clone()) {
                events.push((entity.clone(), side));
            }
        }
    }
    // Changes the anchor does not hold that are older than it, from the side
    // whose anchor it is not: applied right after it, in their own order.
    let mut stale: Vec<(i64, String)> = Vec::new();
    if let Some(anchor) = &anchor_body {
        for (entity, side) in &events {
            if Some(*side) == anchor_from {
                continue;
            }
            let body: AffectEventBody = entity.body_as()?;
            let change = Change::of(entity.id.clone(), &body);
            if !after(anchor, &change) {
                stale.push((body.at, entity.id.clone()));
            }
        }
    }
    stale.sort();
    let stamp: BTreeMap<String, i64> = stale
        .iter()
        .enumerate()
        .map(|(index, (_, id))| (id.clone(), index as i64 + 1))
        .collect();
    let mut changes: Vec<Change> = Vec::new();
    for (entity, side) in events {
        let mut body: AffectEventBody = entity.body_as()?;
        let mut entity = entity;
        if let (Some(anchor), Some(offset)) = (&anchor_body, stamp.get(&entity.id)) {
            body.at = anchor.at + offset;
            body.late = true;
            late.push(entity.id.clone());
            entity = canonicalize(EntityVersion {
                body: serde_json::to_value(&body).map_err(SyncError::from_json)?,
                digest: String::new(),
                ..entity
            })?;
            out.put(entity.clone(), None);
        } else {
            out.put(entity.clone(), Some(side));
        }
        changes.push(Change::of(entity.id.clone(), &body));
    }
    let standing = match &anchor_body {
        Some(anchor) => {
            let folded = affect::fold(anchor, &changes, i64::MAX)?;
            Some(Standing {
                affect: folded.affect,
                at: folded.at,
            })
        }
        None => None,
    };

    // What the host writes, and what it no longer keeps.
    let mut writes = Vec::new();
    for ((kind, id), entity) in &out.merged {
        match host.get(&(*kind, id.clone())) {
            Some(kept) if kept.digest == entity.digest && kept.state == entity.state => {}
            kept => writes.push(Write {
                kind: *kind,
                id: id.clone(),
                replaces: kept.is_some(),
                from: out.from.get(&(*kind, id.clone())).copied().flatten(),
            }),
        }
    }
    let removes: Vec<Write> = renamed
        .iter()
        .filter(|renamed| renamed.side == Side::Host)
        .filter(|renamed| {
            !out.merged
                .contains_key(&(renamed.kind, renamed.from.clone()))
        })
        .map(|renamed| Write {
            kind: renamed.kind,
            id: renamed.from.clone(),
            replaces: true,
            from: Some(Side::Host),
        })
        .collect();
    let mut counts: BTreeMap<Kind, Count> = BTreeMap::new();
    for entity in host.values() {
        counts.entry(entity.kind).or_default().host += 1;
    }
    for entity in other.values() {
        counts.entry(entity.kind).or_default().other += 1;
    }
    for entity in out.merged.values() {
        counts.entry(entity.kind).or_default().merged += 1;
    }
    late.sort();
    Ok(MergePlan {
        same_name,
        names,
        subject,
        entities: out.merged.into_values().collect(),
        writes,
        removes,
        fields,
        renamed,
        late,
        dropped,
        counts,
        standing,
    })
}

/// Whether a change is past what the anchor holds (`affect::after_anchor`).
fn after(anchor: &AffectAnchorBody, change: &Change) -> bool {
    match &anchor.through_event {
        Some(through) => (change.at, change.id.as_str()) > (anchor.at, through.as_str()),
        None => change.at >= anchor.at,
    }
}

/// A proposal the other side made and that is still open runs where it
/// was made: on that device (H3d-10).
fn for_this_device(
    entity: &EntityVersion,
    device: Option<&Device>,
) -> Result<EntityVersion, SyncError> {
    let Some(device) = device else {
        return Ok(entity.clone());
    };
    let mut body: IntentionBody = entity.body_as()?;
    if body.status != "proposed" {
        return Ok(entity.clone());
    }
    let Some(proposal) = body.proposal.as_mut().and_then(Value::as_object_mut) else {
        return Ok(entity.clone());
    };
    if proposal.get("place").is_some_and(|place| !place.is_null()) {
        return Ok(entity.clone());
    }
    proposal.insert("place".into(), json!("mac"));
    proposal.insert("device_id".into(), json!(device.id));
    if let Some(name) = &device.name {
        proposal.insert("device_name".into(), json!(name));
    }
    canonicalize(EntityVersion {
        body: serde_json::to_value(&body).map_err(SyncError::from_json)?,
        digest: String::new(),
        ..entity.clone()
    })
}

/// One conversation as both sides kept it: every line once (by who said
/// what when; a side may have numbered it anew), oldest first.
fn lines_together(
    ours: ConversationBody,
    theirs: ConversationBody,
    newer: Side,
) -> ConversationBody {
    let key = |line: &ConversationLine| (line.role.clone(), line.at, line.content.clone());
    let had: BTreeSet<_> = ours.lines.iter().map(key).collect();
    let ids: BTreeSet<String> = ours.lines.iter().map(|line| line.id.clone()).collect();
    let mut lines = ours.lines.clone();
    for mut line in theirs.lines.iter().cloned() {
        if had.contains(&key(&line)) {
            continue;
        }
        if ids.contains(&line.id) {
            line.id = format!("{}~", line.id);
        }
        lines.push(line);
    }
    lines.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
    ConversationBody {
        title: match newer {
            Side::Other => theirs.title.or(ours.title),
            Side::Host => ours.title.or(theirs.title),
        },
        created_at: ours.created_at.min(theirs.created_at),
        last_active_at: ours.last_active_at.max(theirs.last_active_at),
        lines,
    }
}

/// Her persona merged, what each field became, and the values that went to
/// her history.
fn merge_persona(
    host: &PersonaBody,
    other: &PersonaBody,
    subject: Option<Side>,
    rev: &Rev,
    now: i64,
) -> Result<(PersonaBody, Vec<FieldOutcome>, Vec<FieldHistoryBody>), SyncError> {
    let mut fields: BTreeMap<String, PersonaField> = BTreeMap::new();
    let mut outcomes = Vec::new();
    let mut history = Vec::new();
    let names: BTreeSet<&String> = host.fields.keys().chain(other.fields.keys()).collect();
    let mut gone = |field: &str, value: &PersonaField, by: &Rev, reason: &str| {
        if !value.value.is_null() {
            history.push(FieldHistoryBody {
                field: field.to_string(),
                value: value.value.clone(),
                rev: value.rev.clone(),
                origin: value.origin.clone(),
                replaced_at: now,
                replaced_by_rev: by.clone(),
                reason: reason.to_string(),
            });
        }
    };
    for name in names {
        let ours = host.fields.get(name);
        let theirs = other.fields.get(name);
        let same = match (ours, theirs) {
            (Some(a), Some(b)) => compared_value(name, &a.value) == compared_value(name, &b.value),
            _ => false,
        };
        let mut outcome = FieldOutcome {
            field: name.clone(),
            from: None,
            both: false,
            to_history: None,
        };
        match subject {
            // Her names differ: the subject's, exactly (H3d-4).
            Some(side) => {
                let (kept, left) = match side {
                    Side::Host => (ours, theirs),
                    Side::Other => (theirs, ours),
                };
                if let Some(kept) = kept {
                    fields.insert(name.clone(), kept.clone());
                    outcome.from = Some(side);
                }
                if let Some(left) = left
                    && !same
                    && !left.value.is_null()
                {
                    let by = kept
                        .map(|kept| kept.rev.clone())
                        .unwrap_or_else(|| rev.clone());
                    gone(name, left, &by, "subject");
                    outcome.to_history = Some(if side == Side::Host {
                        Side::Other
                    } else {
                        Side::Host
                    });
                }
            }
            // One her: the later value, or both lists' items.
            None => match (ours, theirs) {
                (Some(a), None) => {
                    fields.insert(name.clone(), a.clone());
                    outcome.from = Some(Side::Host);
                }
                (None, Some(b)) => {
                    fields.insert(name.clone(), b.clone());
                    outcome.from = Some(Side::Other);
                }
                (Some(a), Some(_)) if same => {
                    fields.insert(name.clone(), a.clone());
                    outcome.from = Some(Side::Host);
                }
                // A field one side leaves unset is not a value (R7): the other's.
                (Some(a), Some(b)) if a.value.is_null() || b.value.is_null() => {
                    let (kept, side) = if a.value.is_null() {
                        (b, Side::Other)
                    } else {
                        (a, Side::Host)
                    };
                    fields.insert(name.clone(), kept.clone());
                    outcome.from = Some(side);
                }
                (Some(a), Some(b)) if field_class(name) == FieldClass::List => {
                    let mut items: Vec<Value> = a.value.as_array().cloned().unwrap_or_default();
                    for item in b.value.as_array().cloned().unwrap_or_default() {
                        if !items.contains(&item) {
                            items.push(item);
                        }
                    }
                    let value = Value::Array(items);
                    if value == a.value {
                        fields.insert(name.clone(), a.clone());
                        outcome.from = Some(Side::Host);
                    } else if value == b.value {
                        fields.insert(name.clone(), b.clone());
                        outcome.from = Some(Side::Other);
                    } else {
                        fields.insert(
                            name.clone(),
                            PersonaField {
                                value,
                                rev: rev.clone(),
                                origin: rev.node.clone(),
                            },
                        );
                        outcome.both = true;
                    }
                }
                (Some(a), Some(b)) => {
                    let (kept, left, side) = if b.rev > a.rev {
                        (b, a, Side::Other)
                    } else {
                        (a, b, Side::Host)
                    };
                    fields.insert(name.clone(), kept.clone());
                    outcome.from = Some(side);
                    if !left.value.is_null() {
                        gone(name, left, &kept.rev, "lww");
                        outcome.to_history = Some(if side == Side::Host {
                            Side::Other
                        } else {
                            Side::Host
                        });
                    }
                }
                (None, None) => {}
            },
        }
        outcomes.push(outcome);
    }
    Ok((PersonaBody { fields }, outcomes, history))
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
