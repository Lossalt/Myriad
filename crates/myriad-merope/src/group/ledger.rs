//! The group's ledger: how it types and how long she takes to answer, as
//! numbers only (see `talk_shape::Typed`). Who is a hash, never an id; no
//! one's words are kept.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::*;

pub const LEDGER_MESSAGES: usize = 2000;
pub const LEDGER_WAITS: usize = 500;
/// Messages of each side the piece counts stay within.
pub const LEDGER_COUNTED: u32 = 2000;

/// The ledger as the end keeps it (this shape is the stored one).
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub messages: Vec<Typed>,
    pub her_waits: Vec<f64>,
    #[serde(default)]
    pub theirs: Counts,
    #[serde(default)]
    pub hers: Counts,
}

/// A member as the ledger knows them: the same token for the same person,
/// never their id. The first six bytes of SHA-256, so every end and every
/// build gives the same token (D2-18).
pub fn ledger_who(from: &str) -> String {
    Sha256::digest(from.as_bytes())[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Add what was noted to what the ledger kept, keeping the latest.
pub fn merge_ledger(
    mut ledger: Ledger,
    messages: Vec<Typed>,
    waits: Vec<f64>,
    theirs: &Counts,
    hers: &Counts,
) -> Ledger {
    ledger.messages.extend(messages);
    let over = ledger.messages.len().saturating_sub(LEDGER_MESSAGES);
    ledger.messages.drain(..over);
    ledger.her_waits.extend(waits);
    let over = ledger.her_waits.len().saturating_sub(LEDGER_WAITS);
    ledger.her_waits.drain(..over);
    ledger.theirs.merge(theirs);
    ledger.theirs.keep_within(LEDGER_COUNTED);
    ledger.hers.merge(hers);
    ledger.hers.keep_within(LEDGER_COUNTED);
    ledger
}

/// Note a message typed there, and how long she took if it was her first
/// answer to someone who called her; whether the ledger is due a write.
pub(super) fn note_said(
    group: &mut Group,
    typed: Option<Typed>,
    waited: Option<f64>,
    said: Option<&str>,
) -> bool {
    if let (Some(typed), Some(said)) = (&typed, said) {
        if typed.by == HER {
            group.ledger_hers.add(said);
        } else {
            group.ledger_theirs.add(said);
        }
    }
    group.ledger.extend(typed);
    group.ledger_waits.extend(waited);
    group.ledger.len() >= LEDGER_EVERY || !group.ledger_waits.is_empty()
}

/// Write what was noted to the ledger.
pub(super) fn keep_ledger(group: &mut Group, out: &mut Outcome) {
    let messages = std::mem::take(&mut group.ledger);
    let waits = std::mem::take(&mut group.ledger_waits);
    let theirs = std::mem::take(&mut group.ledger_theirs);
    let hers = std::mem::take(&mut group.ledger_hers);
    if messages.is_empty() && waits.is_empty() {
        return;
    }
    out.effects.push(Effect::Ledger {
        messages,
        waits,
        theirs,
        hers,
    });
}

/// How her lines differ from the people's here: from what was noted
/// (`noted`, taken when she asked) and what the ledger kept; while the
/// ledger has too little of theirs, from the lines in mind.
pub(super) fn differs(group: &Group, noted: &Pieces, stored: Option<&Pieces>) -> Option<String> {
    use crate::contrast::{describe, overused};
    let (mut theirs, mut hers) = (noted.theirs.clone(), noted.hers.clone());
    if let Some(stored) = stored {
        theirs.merge(&stored.theirs);
        hers.merge(&stored.hers);
    }
    if theirs.messages < 20 {
        let kept = |hers: bool| {
            Counts::of(
                group
                    .lines
                    .iter()
                    .filter(|line| line.hers == hers)
                    .flat_map(|line| line.text.lines()),
            )
        };
        (theirs, hers) = (kept(false), kept(true));
    }
    describe(&overused(&hers, &theirs))
}
