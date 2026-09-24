//! Wait for the browser to run query_windows / music_get_status and send the
//! live snapshot back before the next recipe step reads `_music_status` /
//! `_window_state`.
//!
//! This is an in-flight oneshot, not WaitingForInput — the user never sees a
//! question card. No SSE listener (sync `process`) skips the wait.

use crate::services::agent::capability::CapabilityRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use once_cell::sync::Lazy;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::services::agent::types::ExecutionContext;

use super::events::StepEventEmitter;

const ACK_TIMEOUT: Duration = Duration::from_secs(5);

struct PendingSender {
    identity: Arc<()>,
    sender: oneshot::Sender<Value>,
}

static PENDING: Lazy<Mutex<HashMap<String, PendingSender>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Owns the registration across both event publication and the snapshot wait.
/// Cancellation at either await point releases the entry synchronously.
pub struct PendingAck {
    key: String,
    identity: Arc<()>,
    receiver: oneshot::Receiver<Value>,
}

impl Drop for PendingAck {
    fn drop(&mut self) {
        let mut pending = lock_pending();
        // The same task/step may already have a newer wait. Only its owner may
        // remove an entry, including when replacement closes the old channel.
        if pending
            .get(&self.key)
            .is_some_and(|entry| Arc::ptr_eq(&entry.identity, &self.identity))
        {
            pending.remove(&self.key);
        }
    }
}

fn ack_key(task_id: &str, step_id: &str) -> String {
    format!("{task_id}\0{step_id}")
}

fn lock_pending() -> std::sync::MutexGuard<'static, HashMap<String, PendingSender>> {
    PENDING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn actions_need_snapshot_ack(output: &Value) -> bool {
    crate::services::agent::collect_step_frontend_actions(std::iter::once(output))
        .iter()
        .any(|action| {
            matches!(
                action.get("type").and_then(Value::as_str),
                Some("query_windows" | "music_get_status")
            )
        })
}

pub fn begin_if_needed(task_id: &str, step_id: &str, output: &Value) -> Option<PendingAck> {
    if !actions_need_snapshot_ack(output) {
        return None;
    }
    let (sender, receiver) = oneshot::channel();
    let pending = PendingAck {
        key: ack_key(task_id, step_id),
        identity: Arc::new(()),
        receiver,
    };
    lock_pending().insert(
        pending.key.clone(),
        PendingSender {
            identity: Arc::clone(&pending.identity),
            sender,
        },
    );
    Some(pending)
}

pub fn submit(task_id: &str, step_id: &str, payload: Value) -> bool {
    lock_pending()
        .remove(&ack_key(task_id, step_id))
        .is_some_and(|entry| entry.sender.send(payload).is_ok())
}

async fn finish(
    pending: Option<PendingAck>,
    capability_id: &str,
    mut output: Value,
    context: &mut ExecutionContext,
) -> Value {
    let Some(mut pending) = pending else {
        return output;
    };
    let ack = match tokio::time::timeout(ACK_TIMEOUT, &mut pending.receiver).await {
        Ok(Ok(value)) => value,
        _ => return output,
    };
    merge_ack(capability_id, &mut output, &ack, context);
    output
}

fn merge_ack(capability_id: &str, output: &mut Value, ack: &Value, context: &mut ExecutionContext) {
    if let Some(music) = ack.get("musicStatus").filter(|value| value.is_object()) {
        context
            .variables
            .insert("_music_status".to_string(), music.clone());
        if capability_id == "music.status" {
            if let (Some(object), Some(status)) = (output.as_object_mut(), music.as_object()) {
                for (key, value) in status {
                    object.insert(key.clone(), value.clone());
                }
                object.insert("available".to_string(), json!(true));
            }
        }
    }
    if let Some(windows) = ack.get("windowState").filter(|value| !value.is_null()) {
        context
            .variables
            .insert("_window_state".to_string(), windows.clone());
        if capability_id == "tapp.windows" {
            let available = windows
                .get("available")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if let Some(object) = output.as_object_mut() {
                object.insert("available".to_string(), json!(available));
                if available {
                    if let Some(list) = windows.get("windows") {
                        object.insert("windows".to_string(), list.clone());
                    }
                    if let Some(active) = windows.get("activeWindowId") {
                        object.insert("activeWindowId".to_string(), active.clone());
                    }
                    if let Some(count) = windows.get("windowCount") {
                        object.insert("windowCount".to_string(), count.clone());
                    }
                    object.insert("message".to_string(), Value::Null);
                }
            }
        }
    }
}

/// Register → optionally emit StepCompleted → wait → merge.
pub async fn publish_and_await_snapshots(
    emitter: &StepEventEmitter,
    task_id: &str,
    step_id: &str,
    capability_id: &str,
    step_index: u32,
    duration_ms: u64,
    output: Value,
    context: &mut ExecutionContext,
    send_visible: bool,
) -> Value {
    // MCP fields are remote data, not platform-issued browser commands. This
    // boundary is shared by Work and saved Recipes, including their resumes.
    if CapabilityRef::parse(&capability_id).is_mcp() {
        if send_visible {
            emitter
                .step_succeeded(
                    step_id,
                    step_index,
                    duration_ms,
                    super::summarize_output(&output),
                    None,
                    vec![],
                )
                .await;
        }
        context.add_output(step_id, output.clone());
        return output;
    }
    let pending = if emitter.is_live() {
        begin_if_needed(task_id, step_id, &output)
    } else {
        None
    };
    if send_visible {
        emitter
            .step_output_succeeded(step_id, step_index, duration_ms, &output)
            .await;
    }
    let merged = finish(pending, capability_id, output, context).await;
    context.add_output(step_id, merged.clone());
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_output() -> Value {
        json!({"available": false, "frontendAction": {"type": "music_get_status"}})
    }

    fn assert_not_pending(task_id: &str) {
        assert!(
            !lock_pending().contains_key(&ack_key(task_id, "snapshot")),
            "finished or cancelled snapshot wait must release its registration"
        );
    }

    #[test]
    fn dropping_registration_releases_pending_ack() {
        let task_id = "ack-drop-registration";
        let pending = begin_if_needed(task_id, "snapshot", &snapshot_output());
        assert!(pending.is_some());
        drop(pending);
        assert_not_pending(task_id);
    }

    #[tokio::test]
    async fn aborting_snapshot_wait_releases_pending_ack() {
        let task_id = "ack-abort-wait";
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let task = tokio::spawn(async move {
            publish_and_await_snapshots(
                &StepEventEmitter::new(Some(tx)),
                task_id,
                "snapshot",
                "music.status",
                0,
                0,
                snapshot_output(),
                &mut ExecutionContext::default(),
                true,
            )
            .await
        });
        rx.recv().await.unwrap();
        assert!(lock_pending().contains_key(&ack_key(task_id, "snapshot")));
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_not_pending(task_id);
    }

    #[tokio::test]
    async fn cancelling_blocked_emission_releases_pending_ack() {
        use std::future::Future;
        use std::task::Poll;

        let task_id = "ack-cancel-emission";
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let emitter = StepEventEmitter::new(Some(tx));
        emitter
            .step_output_succeeded("already-buffered", 0, 0, &json!({}))
            .await;
        let mut context = ExecutionContext::default();
        let mut publish = Box::pin(publish_and_await_snapshots(
            &emitter,
            task_id,
            "snapshot",
            "music.status",
            0,
            0,
            snapshot_output(),
            &mut context,
            true,
        ));
        std::future::poll_fn(|cx| {
            assert!(publish.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert!(lock_pending().contains_key(&ack_key(task_id, "snapshot")));
        drop(publish);
        assert_not_pending(task_id);
    }

    #[tokio::test]
    async fn submitted_snapshot_is_merged_and_releases_pending_ack() {
        let task_id = "ack-submit";
        let output = snapshot_output();
        let pending = begin_if_needed(task_id, "snapshot", &output);
        assert!(submit(
            task_id,
            "snapshot",
            json!({"musicStatus": {"isPlaying": true}})
        ));
        let mut context = ExecutionContext::default();
        let merged = finish(pending, "music.status", output, &mut context).await;
        assert_eq!(merged["available"], true);
        assert_eq!(merged["isPlaying"], true);
        assert_eq!(context.variables["_music_status"]["isPlaying"], true);
        assert_not_pending(task_id);
        assert!(!submit(task_id, "snapshot", json!({})));
    }

    #[tokio::test]
    async fn timed_out_snapshot_preserves_output_and_releases_pending_ack() {
        let task_id = "ack-timeout";
        let output = snapshot_output();
        let pending = begin_if_needed(task_id, "snapshot", &output);
        let mut context = ExecutionContext::default();
        let merged = finish(pending, "music.status", output.clone(), &mut context).await;
        assert_eq!(merged, output);
        assert!(!context.variables.contains_key("_music_status"));
        assert_not_pending(task_id);
    }

    #[tokio::test]
    async fn replaced_wait_cannot_remove_new_registration() {
        let task_id = "ack-replace-wait";
        let output = snapshot_output();
        let old = begin_if_needed(task_id, "snapshot", &output);
        let replacement = begin_if_needed(task_id, "snapshot", &output);
        let mut context = ExecutionContext::default();
        assert_eq!(
            finish(old, "music.status", output.clone(), &mut context).await,
            output
        );
        assert!(submit(
            task_id,
            "snapshot",
            json!({"musicStatus": {"isPlaying": true}})
        ));
        let merged = finish(replacement, "music.status", output, &mut context).await;
        assert_eq!(merged["isPlaying"], true);
        assert_not_pending(task_id);
    }

    #[test]
    fn query_and_music_status_need_ack() {
        assert!(actions_need_snapshot_ack(&json!({
            "frontendAction": { "type": "music_get_status", "timestamp": 1 }
        })));
        assert!(actions_need_snapshot_ack(&json!({
            "frontendAction": { "type": "query_windows", "timestamp": 1 }
        })));
        assert!(!actions_need_snapshot_ack(&json!({
            "frontendAction": { "type": "navigate", "path": "/phantasi", "timestamp": 1 }
        })));
    }

    #[test]
    fn merge_writes_live_music_into_status_output() {
        let mut output = json!({
            "available": false,
            "frontendAction": { "type": "music_get_status" }
        });
        let mut context = ExecutionContext::default();
        merge_ack(
            "music.status",
            &mut output,
            &json!({ "musicStatus": { "isPlaying": true, "isEnabled": true } }),
            &mut context,
        );
        assert_eq!(output["available"], true);
        assert_eq!(output["isPlaying"], true);
        assert_eq!(context.variables["_music_status"]["isPlaying"], true);
    }
}
