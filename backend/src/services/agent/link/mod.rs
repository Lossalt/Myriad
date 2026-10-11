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

mod connection;
mod contract;
mod turns;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use tokio::sync::oneshot;

pub(crate) use connection::serve;
pub(crate) use contract::Close;

/// The largest frame a child may send.
pub(crate) fn max_frame_bytes() -> usize {
    contract::CONTRACT.limits.max_frame_bytes
}

struct Open {
    id: u64,
    close: oneshot::Sender<Close>,
}

/// The open link of each device, by device id.
static OPEN: LazyLock<Mutex<HashMap<String, Open>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Registers a new link for the device, closing the one it had.
fn open(device_id: &str) -> (u64, oneshot::Receiver<Close>) {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (close, closed) = oneshot::channel();
    if let Ok(mut open) = OPEN.lock() {
        if let Some(previous) = open.insert(device_id.to_string(), Open { id, close }) {
            let _ = previous.close.send(Close::Replaced);
        }
    }
    (id, closed)
}

/// Forgets the link `id` when it is still the device's.
fn forget(device_id: &str, id: u64) {
    if let Ok(mut open) = OPEN.lock() {
        if open.get(device_id).is_some_and(|open| open.id == id) {
            open.remove(device_id);
        }
    }
}

/// Closes the device's link, if it has one open, saying why.
pub(crate) fn disconnect(device_id: &str, why: Close) {
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
