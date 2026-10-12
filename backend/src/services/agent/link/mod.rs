//! Her link: the one WebSocket a child end dials to her host,
//! `GET /api/agent/link` with a device token. Its frames, codes and timings
//! are `shared/her_link_contract.json`, which children build against.
//!
//! Here the site is her host and the child is a native app on the site
//! owner's device. A child sends what the person says and whether they are
//! there; the host runs her as it always does and sends the run, her acting
//! and her mood down. Nothing she is — persona, memory, mood, decisions,
//! model keys — goes to the child, and no frame or error carries a secret.
//! Runs start for the site owner through the same entry and gate as every
//! other channel, so what a turn may do is the owner's granted permissions
//! as they stand at that moment.
//!
//! One link per device: a new connection replaces the old one. Removing the
//! device, or the site getting another owner, closes it.
//!
//! What she says unprompted goes to the app when that is where the person
//! is using her ([`say`]); see `merope::ingest` and `merope::reach`.
//!
//! Her body on the app ([`body`]): a child that says it takes `playback`
//! gets her director's refinements as the reply is being said, and reports
//! how far saying it got; one that takes `touch` sends contact with her
//! figure up, and she answers it there. One that takes `wardrobe` wears its
//! own set of her wardrobe ([`outfit`]).
//!
//! Her senses on the app ([`senses`]): what a child reports seeing and
//! hearing for her, as far as it said it senses it, is what she senses of
//! the person while the app is where they are using her.
//!
//! Her experience on the app ([`experience`]): Work a child that takes
//! `experience` runs starts from her Work memories, asked for over the
//! link, and what it did comes back up to become part of her.

mod body;
mod connection;
mod contract;
mod experience;
mod outfit;
mod say;
mod senses;
mod turns;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use tokio::sync::{mpsc, oneshot};

pub(crate) use body::perform_say;
pub(crate) use connection::serve;
pub(crate) use contract::Close;
pub(crate) use say::{Say, SayKind, can_say, say};

/// The largest frame a child may send.
pub(crate) fn max_frame_bytes() -> usize {
    contract::CONTRACT.limits.max_frame_bytes
}

struct Open {
    id: u64,
    owner: i32,
    close: oneshot::Sender<Close>,
    /// Where her `say` frames go, once the child said it takes them.
    says: Option<mpsc::Sender<connection::Outgoing>>,
    /// The child takes `perform` refinements of what it is saying.
    playback: bool,
    /// Where `experience.taken` goes, once the child said it sends `experience`.
    experience: Option<mpsc::Sender<connection::Outgoing>>,
}

/// The open link of each device, by device id.
static OPEN: LazyLock<Mutex<HashMap<String, Open>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Registers a new link for the device, closing the one it had.
fn open(device_id: &str, owner: i32) -> (u64, oneshot::Receiver<Close>) {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (close, closed) = oneshot::channel();
    if let Ok(mut open) = OPEN.lock() {
        let link = Open {
            id,
            owner,
            close,
            says: None,
            playback: false,
            experience: None,
        };
        if let Some(previous) = open.insert(device_id.to_string(), link) {
            let _ = previous.close.send(Close::Replaced);
        }
    }
    (id, closed)
}

/// The link `id` takes `say` frames, sent through `sender`, and with
/// `playback` the refinements of how she acts them.
fn takes_say(device_id: &str, id: u64, sender: mpsc::Sender<connection::Outgoing>, playback: bool) {
    if let Ok(mut open) = OPEN.lock() {
        if let Some(link) = open.get_mut(device_id).filter(|link| link.id == id) {
            link.says = Some(sender);
            link.playback = playback;
        }
    }
}

/// The link `id` sends `experience`: what is written of it is answered
/// through `sender`, whichever link carried it.
fn takes_experience(device_id: &str, id: u64, sender: mpsc::Sender<connection::Outgoing>) {
    if let Ok(mut open) = OPEN.lock() {
        if let Some(link) = open.get_mut(device_id).filter(|link| link.id == id) {
            link.experience = Some(sender);
        }
    }
}

/// Where the owner's device is told an experience was written, when its
/// link is open and sends them.
fn experience_sender(owner: i32, device_id: &str) -> Option<mpsc::Sender<connection::Outgoing>> {
    OPEN.lock().ok().and_then(|open| {
        open.get(device_id)
            .filter(|link| link.owner == owner)
            .and_then(|link| link.experience.clone())
    })
}

/// Where the owner's device takes `say` frames, when its link is open and does.
fn say_sender(owner: i32, device_id: &str) -> Option<mpsc::Sender<connection::Outgoing>> {
    OPEN.lock().ok().and_then(|open| {
        open.get(device_id)
            .filter(|link| link.owner == owner)
            .and_then(|link| link.says.clone())
    })
}

/// Where the owner's device takes refinements of her `say` lines, when its
/// link is open and does.
fn playback_sender(owner: i32, device_id: &str) -> Option<mpsc::Sender<connection::Outgoing>> {
    OPEN.lock().ok().and_then(|open| {
        open.get(device_id)
            .filter(|link| link.owner == owner && link.playback)
            .and_then(|link| link.says.clone())
    })
}

/// The device has a link open now: what the owner's device list shows as
/// connected.
pub(crate) fn is_open(device_id: &str) -> bool {
    OPEN.lock()
        .map(|open| open.contains_key(device_id))
        .unwrap_or(false)
}

/// Forgets the link `id` when it is still the device's.
fn forget(device_id: &str, id: u64) {
    if let Ok(mut open) = OPEN.lock() {
        if open.get(device_id).is_some_and(|open| open.id == id) {
            open.remove(device_id);
        }
    }
}

/// Closes the device's link, if it has one open, saying why. A device that
/// is not one any more keeps nothing waiting for it.
pub(crate) fn disconnect(device_id: &str, why: Close) {
    if matches!(why, Close::Revoked | Close::OwnerChanged) {
        say::forget_device(device_id);
    }
    let open = OPEN.lock().ok().and_then(|mut open| open.remove(device_id));
    if let Some(open) = open {
        let _ = open.close.send(why);
    }
}

/// Closes every open link: the host is stopping.
pub(crate) fn disconnect_all(why: Close) {
    let open: Vec<Open> = OPEN
        .lock()
        .map(|mut open| open.drain().map(|(_, open)| open).collect())
        .unwrap_or_default();
    for open in open {
        let _ = open.close.send(why);
    }
}
