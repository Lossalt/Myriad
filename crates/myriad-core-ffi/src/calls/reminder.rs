//! When a reminder is due (`myriad_agent_rules::reminder`): the site's
//! reminders and a Mac's come due by the same rule.

use myriad_agent_rules::reminder;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Zone, at, input, on_zone, opt_at};
use crate::Failure;

fn micros(at: Option<chrono::DateTime<chrono::Utc>>) -> Value {
    json!(at.map(|at| at.timestamp_micros()))
}

/// `datetime` and `repeat` as `reminder.create` stored them, how far it was
/// reminded of (`remindedThrough`), now, and the zone a time with no offset
/// is read in. Answers `due` (the time to remind of now, or null) and
/// `latest` (the latest time it comes at, at or before now).
pub(super) fn due(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        datetime: String,
        repeat: String,
        #[serde(default)]
        reminded_through: Option<i64>,
        now: i64,
        zone: String,
    }
    let req: In = input(raw)?;
    let (now, through, zone) = (
        at(req.now)?,
        opt_at(req.reminded_through)?,
        Zone::parse(&req.zone)?,
    );
    let (due, latest) = on_zone!(zone, |zone| (
        reminder::due(&req.datetime, &req.repeat, through, now, &zone),
        reminder::latest(&req.datetime, &req.repeat, now, &zone),
    ));
    Ok(json!({ "due": micros(due), "latest": micros(latest) }))
}
