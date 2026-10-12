//! The site's notifications about her, on the app: the `notice` frame. Only
//! a few are hers to bring there — Work she did that is done, failed or was
//! cancelled, Work waiting for the owner's reply, and a report of hers that
//! is ready — and only to a child that said it shows notifications
//! (`notify`). What she says to them on her own already comes as `say`, so
//! her reaching out is not one of them.

use std::collections::VecDeque;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::services::agent::notifications::{Notification, NotificationEvent};

use super::connection::Outgoing;
use super::contract::valid_id;

/// The kind of notice a site event is on the app, for the ones that go there.
pub(super) fn kind_of(event_key: &str) -> Option<&'static str> {
    match event_key {
        "agent.task_completed" | "agent.task_failed" | "agent.task_cancelled" => Some("work"),
        "agent.clarification" => Some("waiting"),
        "agent.merope.report_ready" => Some("report"),
        _ => None,
    }
}

/// The notification's id as the contract's id pattern takes it: its own
/// when it fits, else a stable digest of it.
fn notice_id(id: &str) -> String {
    if valid_id(id) {
        return id.to_string();
    }
    let digest = Sha256::digest(id.as_bytes());
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("ntc_{hex}")
}

/// The `notice` for one of the owner's notifications, when it is one the app shows.
pub(super) fn notice(notification: &Notification, owner: i32) -> Option<Outgoing> {
    if notification.user_id != Some(owner) {
        return None;
    }
    let event_key = notification.event_key()?;
    let kind = kind_of(event_key)?;
    if notification.title.trim().is_empty() && notification.body.trim().is_empty() {
        return None;
    }
    Some(Outgoing::latest(
        "notice",
        json!({
            "id": notice_id(&notification.id),
            "kind": kind,
            "eventKey": event_key,
            "title": notification.title,
            "body": notification.body,
            "at": notification.created_at.timestamp_millis(),
        }),
    ))
}

/// Notices already sent on one link: an updated notification comes again
/// with the same id, and is shown once per id and event.
#[derive(Default)]
pub(super) struct Sent {
    seen: VecDeque<(String, String)>,
}

impl Sent {
    const KEPT: usize = 256;

    /// The notice for `event`, the first time it comes for the owner.
    pub(super) fn first(&mut self, event: &NotificationEvent, owner: i32) -> Option<Outgoing> {
        let NotificationEvent::NewNotification { notification } = event else {
            return None;
        };
        let outgoing = notice(notification, owner)?;
        let key = (
            notification.id.clone(),
            notification.event_key().unwrap_or_default().to_string(),
        );
        if self.seen.contains(&key) {
            return None;
        }
        self.seen.push_back(key);
        while self.seen.len() > Self::KEPT {
            self.seen.pop_front();
        }
        Some(outgoing)
    }
}
