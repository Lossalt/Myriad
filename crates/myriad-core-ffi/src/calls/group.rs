//! Her mind in a group (`myriad_merope::group`): one step of it, and the
//! rules an end needs around it, so a Mac hosting her runs the same group
//! behaviour as the site (D2). See `shared/group_mind_contract.json`.

use myriad_merope::contrast::Counts;
use myriad_merope::group::{self, At, Event, Group, Her, Ledger, Line, Rolls};
use myriad_merope::talk_shape::Typed;
use serde::Deserialize;
use serde_json::{Value, json};

use super::input;
use crate::Failure;

/// The group's state as last handed back (none for a group she has nothing
/// in mind of), now, chance (`rolls` first, then from `seed`), how she is,
/// and what came. Answers the state to keep, what to do, and for a line
/// heard whether it was new.
pub(super) fn step(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        #[serde(default)]
        state: Option<Value>,
        now: i64,
        seed: u64,
        #[serde(default)]
        rolls: Vec<f64>,
        her: Her,
        event: Event,
    }
    let req: In = input(raw)?;
    let mut group = req.state.map(Group::read).unwrap_or_default();
    let mut at = At {
        now: req.now,
        her: req.her,
        rolls: Rolls::new(req.seed, req.rolls),
    };
    let out = group::step(&mut group, &mut at, req.event);
    let mut answer = json!({ "state": group, "effects": out.effects });
    if let Some(fresh) = out.fresh {
        answer["fresh"] = json!(fresh);
    }
    if let Some(taken) = out.taken {
        answer["taken"] = json!(taken);
    }
    Ok(answer)
}

/// The group as she weighs saying something first in it, or null.
pub(super) fn offer(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        state: Value,
        now: i64,
    }
    let req: In = input(raw)?;
    Ok(json!({ "offer": group::offer(&Group::read(req.state), req.now) }))
}

/// The group's ledger as kept (none when nothing is), with what a `ledger`
/// effect adds.
pub(super) fn merge_ledger(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        #[serde(default)]
        stored: Option<Ledger>,
        messages: Vec<Typed>,
        waits: Vec<f64>,
        theirs: Counts,
        hers: Counts,
    }
    let req: In = input(raw)?;
    let ledger = group::merge_ledger(
        req.stored.unwrap_or_default(),
        req.messages,
        req.waits,
        &req.theirs,
        &req.hers,
    );
    Ok(json!({ "ledger": ledger }))
}

/// A member as the ledger knows them.
pub(super) fn ledger_who(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        from: String,
    }
    let req: In = input(raw)?;
    Ok(json!({ "who": group::ledger_who(&req.from) }))
}

/// Lines kept of a group, as its talk read back for `resumed`.
pub(super) fn kept_as_heard(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        platform: String,
        chat: String,
        #[serde(default)]
        thread: Option<i64>,
        lines: Vec<Line>,
    }
    let req: In = input(raw)?;
    Ok(json!({
        "past": group::kept_as_heard(&req.platform, &req.chat, req.thread, req.lines),
    }))
}
