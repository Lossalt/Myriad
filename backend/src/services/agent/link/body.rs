//! Her body on the app: her director's refinements of what the app is
//! saying, how far saying it got, and contact with her figure there.
//!
//! - A child that takes `playback` follows each of its runs' playback
//!   direction (the same slot the site's face reads over HTTP): every new
//!   version goes down as a `perform` with `live`, also after the run's end,
//!   until the slot closes. It reports where saying the reply has got to
//!   (`playback.observe`), when it is over (`playback.end`), and how much of
//!   it was said aloud (`speech.delivered`), which marks a cut-off reply in
//!   her history. Her lines on the app (`say`) are refined once and sent as
//!   a `perform` with that line's `sayId`.
//! - A child that takes `touch` sends contact with her figure up: while it
//!   goes on (`touch.appraise`, answered `touch.reaction`) and once it ends
//!   (`touch`). Both go through `merope::touch`, the site's own limits and
//!   rules, with the app's face as the one touched; she answers on it.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::services::agent::merope::api::touch::TouchSummary;
use crate::services::agent::run_hub::AgentRun;

use super::connection::Outgoing;
use super::turns::Turn;

/// What `playback.observe` may carry, as the site's
/// `PUT /runs/{id}/performance` takes it.
pub(super) const UPCOMING_BYTES: usize = 7_200;
pub(super) const RIG_BYTES: usize = 16_384;

/// Why the child's `by` says a line was cut off.
const CUT_OFF_BY: [&str; 4] = ["barge-in", "stop", "new-turn", "hidden"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Observe {
    pub(super) run_id: String,
    pub(super) upcoming_text: String,
    pub(super) rig: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct End {
    pub(super) run_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) say_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Delivered {
    pub(super) spoken_chars: u64,
    pub(super) total_chars: u64,
    pub(super) interrupted: bool,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) turn_id: Option<String>,
    #[serde(default)]
    pub(super) run_id: Option<String>,
    #[serde(default)]
    pub(super) say_id: Option<String>,
    #[serde(default)]
    pub(super) by: Option<String>,
}

impl Delivered {
    /// Why it was cut off, when it says one the contract knows.
    pub(super) fn by(&self) -> Option<&str> {
        self.by.as_deref().filter(|by| CUT_OFF_BY.contains(by))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Appraise {
    pub(super) touch_id: String,
    pub(super) touch: TouchSummary,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) rig_state: Option<Value>,
}

#[derive(Deserialize)]
pub(super) struct Touched {
    pub(super) touch: TouchSummary,
}

/// Her director's refinement of the turn's reply, version `live` of its
/// playback direction.
pub(super) fn live_perform(turn: &Turn, live: u64, directive: Value) -> Outgoing {
    Outgoing::latest(
        "perform",
        json!({
            "turnId": turn.turn_id,
            "runId": turn.run_id,
            "live": live,
            "directive": directive,
        }),
    )
}

/// Sends the run's playback direction as it changes, until it closes. A
/// direction already closed when first read is a reply already said: she
/// does not act it again.
pub(super) async fn follow_direction(tx: mpsc::Sender<Outgoing>, turn: Turn, run: Arc<AgentRun>) {
    let slot = run.playback_direction.clone();
    let mut after = 0;
    let mut first = true;
    loop {
        let state = slot.read_after(after).await;
        if first && state.closed {
            return;
        }
        first = false;
        if state.version > after {
            after = state.version;
            let directive = state
                .performance
                .as_ref()
                .and_then(|performance| serde_json::to_value(performance).ok());
            if let Some(directive) = directive {
                if tx
                    .send(live_perform(&turn, after, directive))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        if state.closed {
            return;
        }
    }
}

/// Sends her director's refinement of the `say` line `say_id` to the owner's
/// app `device_id`, when its link is open and takes it. Expendable: a full
/// queue drops it, as motion is under transport pressure.
pub(crate) fn perform_say(user_id: i32, device_id: &str, say_id: &str, directive: Value) -> bool {
    let Some(sender) = super::playback_sender(user_id, device_id) else {
        return false;
    };
    sender
        .try_send(Outgoing::latest(
            "perform",
            json!({ "sayId": say_id, "directive": directive }),
        ))
        .is_ok()
}

/// What touch.reaction answers.
pub(super) fn reaction(touch_id: &str, reaction: Option<Value>) -> Outgoing {
    let reaction = reaction
        .and_then(|value| value.get("reaction").cloned())
        .filter(Value::is_string)
        .unwrap_or(Value::Null);
    Outgoing::latest(
        "touch.reaction",
        json!({ "touchId": touch_id, "reaction": reaction }),
    )
}
