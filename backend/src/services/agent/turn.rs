//! Turn identity and Chat supersession.
//!
//! `runId` is the durable root of a Chat/Work turn. Do not invent a second
//! `turnId`. `generation` lives only in memory: a newer Chat request replaces
//! the previous Chat run's text, speech, and motion. Work is never cancelled
//! by Chat. SSE disconnect unsubscribes; it does not cancel the run.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use once_cell::sync::Lazy;
use serde_json::json;
use tokio::sync::{Mutex, oneshot};

use super::types::AgentProgressEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPlane {
    /// Lifecycle and semantic events. These may use the run hub.
    Control,
    /// Volume, visemes, spectrum, VAD, phonemes. Must never enter the run hub.
    Data,
}

/// Exhaustive: a new `AgentProgressEvent` variant must pick a plane.
pub fn event_plane(event: &AgentProgressEvent) -> EventPlane {
    match event {
        AgentProgressEvent::RunStarted { .. }
        | AgentProgressEvent::WorkPlanUpdated { .. }
        | AgentProgressEvent::TaskCreated { .. }
        | AgentProgressEvent::StepStarted { .. }
        | AgentProgressEvent::StepCompleted { .. }
        | AgentProgressEvent::Progress { .. }
        | AgentProgressEvent::TaskCompleted { .. }
        | AgentProgressEvent::WaitingForInput { .. }
        | AgentProgressEvent::SessionCreated { .. }
        | AgentProgressEvent::SessionTitleUpdated { .. }
        | AgentProgressEvent::SummaryToken { .. }
        | AgentProgressEvent::ThinkingToken { .. }
        | AgentProgressEvent::PerformancePlan { .. }
        | AgentProgressEvent::MeropeStateChanged { .. }
        | AgentProgressEvent::OutfitOverlay { .. }
        | AgentProgressEvent::MusicControl { .. }
        | AgentProgressEvent::Error { .. } => EventPlane::Control,
    }
}

pub const TURN_SUPERSEDED_CODE: &str = "TURN_SUPERSEDED";

/// Chat completions are spoken lines, not Work outcomes. They must not become
/// persona events, mood bumps, or task notifications.
pub fn is_chat_turn_completion(response: &serde_json::Value) -> bool {
    if response.get("code").and_then(|value| value.as_str()) == Some(TURN_SUPERSEDED_CODE) {
        return true;
    }
    let Some(data) = response.get("data") else {
        return false;
    };
    data.get("mode").and_then(|value| value.as_str()) == Some("chat")
        || data.get("type").and_then(|value| value.as_str()) == Some("chat")
}

struct ChatTurnSlot {
    tx: oneshot::Sender<()>,
    slot_id: u64,
    run_id: String,
    spoken: Spoken,
}

const RUNNING: u8 = 0;
const FINISHED: u8 = 1;
/// Cut off by a newer turn, which saves what was said.
const TAKEN: u8 = 2;
/// Cut off with no newer turn: the turn saves what it said itself.
const STOPPED: u8 = 3;
const MAX_SPOKEN_BYTES: usize = 16 * 1024;

/// What a Chat turn has said so far. A turn either finishes and saves its
/// whole reply, or is cut off and only what was said is saved; never both.
#[derive(Clone, Default)]
pub struct Spoken {
    text: Arc<std::sync::Mutex<String>>,
    state: Arc<AtomicU8>,
    /// Spoken aloud: the text runs ahead of what they heard.
    voice: bool,
}

/// A turn cut off partway, and what it had said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutOff {
    pub text: String,
    pub voice: bool,
    pub run_id: String,
}

impl Spoken {
    pub fn push(&self, token: &str) {
        if let Ok(mut text) = self.text.lock() {
            if text.len() + token.len() <= MAX_SPOKEN_BYTES {
                text.push_str(token);
            }
        }
    }

    /// The turn ends by itself. False when it was cut off first: then its
    /// reply is not saved, only what it had said.
    pub fn finish(&self) -> bool {
        match self
            .state
            .compare_exchange(RUNNING, FINISHED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(state) => state == FINISHED,
        }
    }

    fn cut(&self, to: u8, run_id: &str) -> Option<CutOff> {
        self.state
            .compare_exchange(RUNNING, to, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let text = self.text.lock().ok()?.trim().to_string();
        (!text.is_empty()).then(|| CutOff {
            text,
            voice: self.voice,
            run_id: run_id.to_owned(),
        })
    }

    /// Stopped with no newer turn: what this turn said, for it to save.
    pub fn stopped(&self) -> Option<String> {
        (self.state.load(Ordering::Acquire) == STOPPED)
            .then(|| self.text.lock().ok().map(|text| text.trim().to_string()))
            .flatten()
            .filter(|text| !text.is_empty())
    }

    pub fn voice(&self) -> bool {
        self.voice
    }
}

/// A claimed Chat turn.
pub struct ChatClaim {
    /// Fires when a newer turn or a stop cuts this one off.
    pub cancelled: oneshot::Receiver<()>,
    pub slot_id: u64,
    /// Where this turn keeps what it has said.
    pub spoken: Spoken,
    /// The previous turn, if this claim cut it off partway.
    pub cut_off: Option<CutOff>,
}

static CHAT_TURNS: Lazy<Mutex<HashMap<(i32, String), ChatTurnSlot>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_CHAT_SLOT: AtomicU64 = AtomicU64::new(1);

/// Register this Chat run as the live turn for the session. The previous Chat
/// turn, if any, is cancelled. Work must not call this.
#[cfg(test)]
pub async fn claim_chat_turn(
    user_id: i32,
    session_id: &str,
    run_id: &str,
) -> (oneshot::Receiver<()>, u64) {
    let claim = claim_speaking_turn(user_id, session_id, run_id, false).await;
    (claim.cancelled, claim.slot_id)
}

/// [`claim_chat_turn`], keeping what the turn says and handing over what the
/// turn it replaces had said, if it was cut off partway.
pub async fn claim_speaking_turn(
    user_id: i32,
    session_id: &str,
    run_id: &str,
    voice: bool,
) -> ChatClaim {
    let (tx, rx) = oneshot::channel();
    let slot_id = NEXT_CHAT_SLOT.fetch_add(1, Ordering::Relaxed);
    let spoken = Spoken {
        voice,
        ..Spoken::default()
    };
    let mut slots = CHAT_TURNS.lock().await;
    let cut_off = slots
        .insert(
            (user_id, session_id.to_string()),
            ChatTurnSlot {
                tx,
                slot_id,
                run_id: run_id.to_owned(),
                spoken: spoken.clone(),
            },
        )
        .and_then(|previous| {
            let cut_off = previous.spoken.cut(TAKEN, &previous.run_id);
            let _ = previous.tx.send(());
            cut_off
        });
    ChatClaim {
        cancelled: rx,
        slot_id,
        spoken,
        cut_off,
    }
}

/// A voice transport may stop its own run, never a newer typed Chat reply.
pub async fn cancel_chat_run(user_id: i32, session_id: &str, run_id: &str) -> bool {
    let mut slots = CHAT_TURNS.lock().await;
    let key = (user_id, session_id.to_owned());
    if !slots.get(&key).is_some_and(|slot| slot.run_id == run_id) {
        return false;
    }
    if let Some(slot) = slots.remove(&key) {
        slot.spoken.cut(STOPPED, &slot.run_id);
        let _ = slot.tx.send(());
    }
    true
}

/// Drop this Chat slot after it finishes, if a newer claim has not replaced it.
pub async fn finish_chat_turn(user_id: i32, session_id: &str, slot_id: u64) {
    let mut slots = CHAT_TURNS.lock().await;
    let key = (user_id, session_id.to_string());
    if slots.get(&key).is_some_and(|slot| slot.slot_id == slot_id) {
        slots.remove(&key);
    }
}

/// Stop the live Chat turn without starting a replacement. Work must not call this.
pub async fn cancel_chat_turn(user_id: i32, session_id: &str) -> bool {
    let mut slots = CHAT_TURNS.lock().await;
    if !session_id.is_empty() {
        if let Some(previous) = slots.remove(&(user_id, session_id.to_string())) {
            previous.spoken.cut(STOPPED, &previous.run_id);
            let _ = previous.tx.send(());
            return true;
        }
        return false;
    }
    let keys: Vec<_> = slots
        .keys()
        .filter(|(uid, _)| *uid == user_id)
        .cloned()
        .collect();
    let mut cancelled = false;
    for key in keys {
        if let Some(previous) = slots.remove(&key) {
            previous.spoken.cut(STOPPED, &previous.run_id);
            let _ = previous.tx.send(());
            cancelled = true;
        }
    }
    cancelled
}

pub fn superseded_turn_event() -> AgentProgressEvent {
    AgentProgressEvent::TaskCompleted {
        task_id: String::new(),
        success: false,
        response: Box::new(json!({
            "success": false,
            "responseType": "error",
            "message": "Replaced by a newer Chat turn",
            "streamTerminal": true,
            "code": TURN_SUPERSEDED_CODE,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn current_progress_events_are_control_plane() {
        let events = [
            AgentProgressEvent::RunStarted {
                run_id: "run_1".into(),
                session_id: None,
            },
            AgentProgressEvent::SummaryToken {
                token: "hi".into(),
                done: false,
            },
            superseded_turn_event(),
        ];
        for event in &events {
            assert_eq!(event_plane(event), EventPlane::Control);
        }
        let encoded = serde_json::to_string(&events[0]).unwrap();
        for forbidden in [
            "viseme",
            "articulation",
            "spectrum",
            "vad",
            "phoneme",
            "volume",
        ] {
            assert!(
                !encoded.contains(forbidden),
                "control event leaked data-plane field {forbidden}"
            );
        }
    }

    #[test]
    fn chat_turn_completion_is_not_a_persona_event() {
        assert!(is_chat_turn_completion(&json!({
            "success": true,
            "message": "你好。",
            "data": { "reply": "你好。", "type": "chat", "mode": "chat" }
        })));
        assert!(is_chat_turn_completion(&json!({
            "success": false,
            "message": "Replaced by a newer Chat turn",
            "streamTerminal": true,
            "code": TURN_SUPERSEDED_CODE,
        })));
        assert!(!is_chat_turn_completion(&json!({
            "success": true,
            "message": "The task finished",
            "data": { "type": "work" }
        })));
    }

    #[test]
    fn superseded_event_is_detectable_and_terminal_shaped() {
        let event = superseded_turn_event();
        match &event {
            AgentProgressEvent::TaskCompleted {
                success, response, ..
            } => {
                assert!(!*success);
                assert_eq!(response["streamTerminal"], true);
                assert_eq!(response["code"], TURN_SUPERSEDED_CODE);
            }
            other => panic!("expected task completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancel_chat_turn_fires_the_live_slot_without_a_replacement() {
        let (first, _) = claim_chat_turn(9, "chat-session", "first").await;
        assert!(cancel_chat_turn(9, "chat-session").await);
        assert!(first.await.is_ok());
        assert!(!cancel_chat_turn(9, "chat-session").await);
    }

    #[tokio::test]
    async fn finish_chat_turn_drops_only_the_matching_slot() {
        let (first, first_id) = claim_chat_turn(8, "chat-session", "first").await;
        let (second, second_id) = claim_chat_turn(8, "chat-session", "second").await;
        assert!(first.await.is_ok());
        finish_chat_turn(8, "chat-session", first_id).await;
        finish_chat_turn(8, "other-session", second_id).await;
        assert!(cancel_chat_turn(8, "chat-session").await);
        drop(second);
        let (_, live_id) = claim_chat_turn(8, "chat-session", "live").await;
        finish_chat_turn(8, "chat-session", live_id).await;
        assert!(!cancel_chat_turn(8, "chat-session").await);
    }

    #[tokio::test]
    async fn newer_chat_cancels_the_previous_chat_once() {
        let (first, _) = claim_chat_turn(12, "chat-session", "first").await;
        let (second, _) = claim_chat_turn(12, "chat-session", "second").await;
        assert!(first.await.is_ok());
        let (third, _) = claim_chat_turn(12, "chat-session", "third").await;
        assert!(second.await.is_ok());
        drop(third);
    }

    #[tokio::test]
    async fn different_sessions_do_not_cancel_each_other() {
        let (mut chat, _) = claim_chat_turn(11, "chat-session", "chat").await;
        let _work = claim_chat_turn(11, "work-session", "work").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut chat)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn repeated_claim_is_idempotent_for_the_latest_turn() {
        let (first, _) = claim_chat_turn(13, "s", "first").await;
        let _second = claim_chat_turn(13, "s", "second").await;
        let _again = claim_chat_turn(13, "s", "again").await;
        assert!(first.await.is_ok());
    }

    #[tokio::test]
    async fn a_cut_off_turn_hands_over_what_it_said_exactly_once() {
        let first = claim_speaking_turn(16, "s", "first", false).await;
        first.spoken.push("我觉得海边");
        first.spoken.push("挺好的，因为");
        let second = claim_speaking_turn(16, "s", "second", false).await;
        assert!(first.cancelled.await.is_ok());
        assert_eq!(
            second.cut_off,
            Some(CutOff {
                text: "我觉得海边挺好的，因为".into(),
                voice: false,
                run_id: "first".into(),
            })
        );
        // The cut-off turn does not also save its whole reply.
        assert!(!first.spoken.finish());
        assert!(first.spoken.stopped().is_none());

        // A turn that already finished is not cut off.
        second.spoken.push("好。");
        assert!(second.spoken.finish());
        let third = claim_speaking_turn(16, "s", "third", true).await;
        assert!(third.cut_off.is_none());

        // Stopped with no newer turn: it keeps what it said, to save itself.
        third.spoken.push("那我们");
        assert!(cancel_chat_run(16, "s", "third").await);
        assert!(!third.spoken.finish());
        assert_eq!(third.spoken.stopped().as_deref(), Some("那我们"));
        assert!(third.spoken.voice());
    }

    #[tokio::test]
    async fn voice_stop_cannot_cancel_the_newer_typed_run() {
        let (voice, _) = claim_chat_turn(14, "shared-chat", "voice-run").await;
        let (typed, _) = claim_chat_turn(14, "shared-chat", "typed-run").await;
        assert!(voice.await.is_ok());
        assert!(!cancel_chat_run(14, "shared-chat", "voice-run").await);
        assert!(!cancel_chat_run(15, "shared-chat", "typed-run").await);
        assert!(cancel_chat_run(14, "shared-chat", "typed-run").await);
        assert!(typed.await.is_ok());
    }
}
