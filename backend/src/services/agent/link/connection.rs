//! One link, from the upgrade to the close: the handshake, what the child
//! sends, the runs it follows, her mood, and the heartbeat that also checks
//! the device is still one.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::services::agent::merope::{self, MoodTransition};
use crate::services::agent::notifications::{NotificationEvent, get_notification_manager};
use crate::services::agent::run::{
    ProcessContext, ProcessRequest, agent_run_envelopes, agent_run_event_is_terminal,
    start_for_user,
};
use crate::services::agent::run_hub::{AgentRun, get_live_run_for_user, get_run_for_user};
use crate::services::agent::{AgentInteractionMode, AgentProgressEvent};
use crate::services::persona_sync::devices::{self, Device, Refused};

use super::contract::{CONTRACT, Close, Timing, valid_id};
use super::turns::{self, Turn};

/// Frames a run follower or the mood follower may have waiting.
const QUEUED_FRAMES: usize = 256;

/// A frame on its way to the child; the link numbers it as it goes out.
pub(super) struct Outgoing {
    pub(super) t: &'static str,
    id: Option<String>,
    pub(super) body: Value,
}

fn frame(t: &'static str, body: Value) -> Outgoing {
    Outgoing { t, id: None, body }
}

/// A frame as a child sends it. Every field but `id` is required.
#[derive(Deserialize)]
pub(super) struct Inbound {
    pub(super) t: String,
    pub(super) seq: u64,
    #[allow(dead_code)] // The host keeps no reliable frames of its own yet.
    ack: u64,
    #[serde(default)]
    pub(super) id: Option<String>,
    #[allow(dead_code)]
    at: i64,
    pub(super) body: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Hello {
    pub(super) protocol: u32,
    pub(super) role: String,
    pub(super) surface: String,
    #[serde(default)]
    pub(super) capabilities: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Input {
    pub(super) surface: String,
    pub(super) text: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
    #[serde(default)]
    pub(super) rig_state: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Resume {
    pub(super) turn_id: String,
    pub(super) after_seq: u64,
    #[serde(default)]
    pub(super) run_id: Option<String>,
}

/// Serves her link to `device` until either end closes it.
pub(crate) async fn serve(socket: WebSocket, db: DatabaseConnection, device: Device) {
    serve_with(socket, db, device, Timing::from_contract()).await;
}

pub(super) async fn serve_with(
    socket: WebSocket,
    db: DatabaseConnection,
    device: Device,
    timing: Timing,
) {
    let (registered, closed) = super::open(&device.id);
    let (sink, stream) = socket.split();
    let (tx, rx) = mpsc::channel(QUEUED_FRAMES);
    let mut link = Link {
        db,
        owner: device.user_id,
        device,
        timing,
        tx,
        queue: Vec::new(),
        sent: 0,
        heard: 0,
        hello: false,
        last_heard: Instant::now(),
        follows: HashMap::new(),
        moods: None,
    };
    tracing::info!(device = %link.device.id, "[link] opened");
    let why = link.run(sink, stream, rx, closed).await;
    // A newer link of the device keeps the presence it reports.
    link.finish(why != Some(Close::Replaced));
    super::forget(&link.device.id, registered);
    tracing::info!(
        device = %link.device.id,
        why = why.map_or("closed by the child", Close::name),
        "[link] closed"
    );
}

struct Link {
    db: DatabaseConnection,
    owner: i32,
    device: Device,
    timing: Timing,
    /// For the run and mood followers.
    tx: mpsc::Sender<Outgoing>,
    /// What the frame just taken answers with, sent before anything else.
    queue: Vec<Outgoing>,
    /// The last seq this end sent, and the highest the child sent.
    sent: u64,
    heard: u64,
    hello: bool,
    last_heard: Instant,
    /// The run each followed turn is being sent from.
    follows: HashMap<String, tokio::task::AbortHandle>,
    moods: Option<tokio::task::AbortHandle>,
}

type Sink = SplitSink<WebSocket, Message>;

impl Link {
    async fn run(
        &mut self,
        mut sink: Sink,
        mut stream: futures::stream::SplitStream<WebSocket>,
        mut rx: mpsc::Receiver<Outgoing>,
        mut closed: oneshot::Receiver<Close>,
    ) -> Option<Close> {
        let hello_by = tokio::time::sleep(self.timing.hello);
        tokio::pin!(hello_by);
        let mut beat = tokio::time::interval_at(
            Instant::now() + self.timing.heartbeat,
            self.timing.heartbeat,
        );
        beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let why = tokio::select! {
                why = &mut closed => Some(why.unwrap_or(Close::Unavailable)),
                incoming = stream.next() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        self.last_heard = Instant::now();
                        self.take(text.as_str()).await
                    }
                    Some(Ok(Message::Binary(_))) => {
                        self.last_heard = Instant::now();
                        self.error("binary_unsupported", None);
                        None
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {
                        self.last_heard = Instant::now();
                        None
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => return None,
                },
                Some(outgoing) = rx.recv() => {
                    self.queue.push(outgoing);
                    None
                }
                () = &mut hello_by, if !self.hello => Some(Close::Timeout),
                _ = beat.tick() => self.beat().await,
            };
            let queued = std::mem::take(&mut self.queue);
            for outgoing in queued {
                if !self.send(&mut sink, outgoing).await {
                    return None;
                }
            }
            if let Some(why) = why {
                let _ = sink
                    .send(Message::Close(Some(CloseFrame {
                        code: why.code(),
                        reason: why.name().into(),
                    })))
                    .await;
                return Some(why);
            }
        }
    }

    async fn send(&mut self, sink: &mut Sink, outgoing: Outgoing) -> bool {
        self.sent += 1;
        let mut envelope = json!({
            "t": outgoing.t,
            "seq": self.sent,
            "ack": self.heard,
            "at": chrono::Utc::now().timestamp_millis(),
            "body": outgoing.body,
        });
        if let Some(id) = outgoing.id {
            envelope["id"] = id.into();
        }
        sink.send(Message::Text(envelope.to_string().into()))
            .await
            .is_ok()
    }

    fn reply(&mut self, outgoing: Outgoing) {
        self.queue.push(outgoing);
    }

    fn error(&mut self, code: &str, frame_seq: Option<u64>) {
        let mut body = json!({ "code": code });
        if let Some(seq) = frame_seq {
            body["ref"] = seq.into();
        }
        self.reply(frame("error", body));
    }

    fn reject(&mut self, turn_id: &str, code: &str) {
        self.reply(frame(
            "run.rejected",
            json!({ "turnId": turn_id, "code": code }),
        ));
    }

    /// A text frame from the child; `Some` closes the link.
    async fn take(&mut self, text: &str) -> Option<Close> {
        let Ok(inbound) = serde_json::from_str::<Inbound>(text) else {
            if !self.hello {
                return Some(Close::BadFrame);
            }
            self.error("bad_frame", None);
            return None;
        };
        self.heard = self.heard.max(inbound.seq);
        if !self.hello {
            if inbound.t != "hello" {
                self.error("not_ready", Some(inbound.seq));
                return None;
            }
            return self.hello(inbound.body).await;
        }
        match inbound.t.as_str() {
            "input" => self.input(inbound).await,
            "presence" => self.presence(inbound).await,
            "run.resume" => self.resume(inbound).await,
            "ping" => self.reply(frame("pong", json!({}))),
            "ack" | "pong" => {}
            t if CONTRACT.frames.contains_key(t) => self.error("bad_frame", Some(inbound.seq)),
            _ => self.error("unknown_frame", Some(inbound.seq)),
        }
        None
    }

    async fn hello(&mut self, body: Value) -> Option<Close> {
        let Ok(hello) = serde_json::from_value::<Hello>(body) else {
            return Some(Close::BadFrame);
        };
        if hello.protocol != CONTRACT.protocol {
            return Some(Close::UnsupportedProtocol);
        }
        match hello.role.as_str() {
            "child" => {}
            // She lives on the site; a device cannot be her host here.
            "host" => return Some(Close::HostIsSite),
            _ => return Some(Close::BadFrame),
        }
        if !CONTRACT.surfaces.contains(&hello.surface) {
            return Some(Close::BadFrame);
        }
        let mut capabilities: Vec<String> = Vec::new();
        for capability in hello.capabilities {
            if CONTRACT.capabilities.contains(&capability) && !capabilities.contains(&capability) {
                capabilities.push(capability);
            }
        }
        self.hello = true;
        let mut told = Vec::new();
        for turn in turns::recent(&self.device.id, turns::TOLD_IN_WELCOME) {
            let Some(run) = get_live_run_for_user(&turn.run_id, self.owner).await else {
                continue;
            };
            let (_, last_sequence, completed) = run.snapshot().await;
            let mut told_turn = json!({
                "turnId": turn.turn_id,
                "runId": turn.run_id,
                "lastSequence": last_sequence,
                "completed": completed,
            });
            if let Some(session_id) = turn.session_id {
                told_turn["sessionId"] = session_id.into();
            }
            told.push(told_turn);
        }
        let welcome = json!({
            "protocol": CONTRACT.protocol,
            "role": "host",
            "host": "site",
            "deviceId": self.device.id,
            "deviceName": self.device.name,
            "lineage": self.device.lineage,
            "hostEpoch": Value::Null,
            "heartbeatMs": self.timing.heartbeat.as_millis() as u64,
            "capabilities": capabilities,
            "turns": told,
            "serverTime": chrono::Utc::now().timestamp_millis(),
        });
        self.reply(frame("welcome", welcome));
        if let Some(mood) = current_mood(&self.db, self.owner).await {
            self.reply(mood);
        }
        if let Some(manager) = get_notification_manager() {
            let events = manager.subscribe();
            let task = tokio::spawn(follow_moods(self.tx.clone(), self.owner, events));
            self.moods = Some(task.abort_handle());
        }
        tracing::info!(device = %self.device.id, "[link] hello");
        None
    }

    async fn input(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(turn_id) = inbound.id else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !valid_id(&turn_id) {
            self.error("bad_id", Some(seq));
            return;
        }
        let Ok(input) = serde_json::from_value::<Input>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        // A resend: the turn already has its run.
        if let Some(turn) = turns::find(&self.db, &self.device.id, &turn_id).await {
            self.reply(accepted(&turn, true));
            return;
        }
        if !CONTRACT.surfaces.contains(&input.surface) {
            self.reject(&turn_id, "surface_unsupported");
            return;
        }
        if input.text.trim().is_empty() {
            self.reject(&turn_id, "empty_input");
            return;
        }
        if input.text.chars().count() > CONTRACT.limits.max_text_chars {
            self.reject(&turn_id, "input_too_long");
            return;
        }
        let request = ProcessRequest {
            input: input.text,
            context: Some(ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: input.session_id,
                rig_state: input.rig_state,
                ..Default::default()
            }),
        };
        let run = match start_for_user(self.db.clone(), self.owner, request).await {
            Ok(run) => run,
            Err(error) => {
                let code = match error.code() {
                    Some(code) if code != "unmapped" => code.to_string(),
                    _ if error.status().is_server_error() => "internal_error".to_string(),
                    _ => "bad_frame".to_string(),
                };
                tracing::info!(device = %self.device.id, %code, "[link] input refused");
                self.reject(&turn_id, &code);
                return;
            }
        };
        // Answered on the device: the site does not notify about it.
        run.answered_elsewhere();
        let turn = Turn {
            turn_id,
            run_id: run.run_id().to_string(),
            session_id: run.session_id().map(str::to_string),
        };
        turns::remember(&self.db, &self.device.id, self.owner, turn.clone()).await;
        self.reply(accepted(&turn, false));
        self.follow(turn, run, 0);
    }

    async fn presence(&mut self, inbound: Inbound) {
        if !inbound.body.get("presence").is_some_and(Value::is_object) {
            self.error("bad_frame", Some(inbound.seq));
            return;
        }
        // As on the site: with her persona off nothing is remembered.
        if merope::is_enabled().await {
            let arrived = crate::services::agent::consciousness::remember_native_presence(
                self.owner,
                &self.device.id,
                &inbound.body,
            );
            // Arriving on the app is arriving: the same greeting as a page.
            if arrived {
                merope::spawn_presence(self.owner);
            }
        }
    }

    async fn resume(&mut self, inbound: Inbound) {
        let Ok(resume) = serde_json::from_value::<Resume>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if !valid_id(&resume.turn_id) {
            self.error("bad_id", Some(inbound.seq));
            return;
        }
        // Only this device's own turns, whatever run id it names.
        let turn = turns::find(&self.db, &self.device.id, &resume.turn_id)
            .await
            .filter(|turn| resume.run_id.as_ref().is_none_or(|id| *id == turn.run_id));
        let Some(turn) = turn else {
            self.reject(&resume.turn_id, "turn_unknown");
            return;
        };
        match get_run_for_user(&turn.run_id, self.owner).await {
            Ok(Some(run)) => self.follow(turn, run, resume.after_seq),
            Ok(None) => self.reject(&resume.turn_id, "turn_unknown"),
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not load a run");
                self.reject(&resume.turn_id, "internal_error");
            }
        }
    }

    /// Sends the turn's run from after `after_seq`, instead of anything that
    /// was already sending it.
    fn follow(&mut self, turn: Turn, run: Arc<AgentRun>, after_seq: u64) {
        let key = turn.turn_id.clone();
        if let Some(previous) = self.follows.remove(&key) {
            previous.abort();
        }
        self.follows.retain(|_, task| !task.is_finished());
        let task = tokio::spawn(follow_run(self.tx.clone(), turn, run, after_seq));
        self.follows.insert(key, task.abort_handle());
    }

    /// Every heartbeat: the child is still there, and the device still one.
    async fn beat(&mut self) -> Option<Close> {
        if self.last_heard.elapsed() > self.timing.silence() {
            return Some(Close::Timeout);
        }
        let owner = match crate::services::principal::site_owner_id(&self.db).await {
            Ok(owner) => owner,
            Err(error) => {
                tracing::warn!(%error, "[link] could not resolve the site owner");
                return None;
            }
        };
        match devices::recheck(&self.db, &self.device.id, owner).await {
            Ok(_) => {}
            Err(Refused::Revoked | Refused::Unknown) => return Some(Close::Revoked),
            Err(Refused::OwnerChanged | Refused::NoOwner) => return Some(Close::OwnerChanged),
            Err(Refused::Db(error)) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not recheck a device");
            }
        }
        if self.hello {
            self.reply(frame("ping", json!({})));
        }
        None
    }

    /// The link is closed: stop sending, and the person is not there now
    /// (unless `gone` is false: another link of the device took over).
    fn finish(&mut self, gone: bool) {
        for (_, task) in self.follows.drain() {
            task.abort();
        }
        if let Some(task) = self.moods.take() {
            task.abort();
        }
        if gone {
            crate::services::agent::consciousness::forget_native_presence(
                self.owner,
                &self.device.id,
            );
        }
    }
}

pub(super) fn accepted(turn: &Turn, duplicate: bool) -> Outgoing {
    let mut body = json!({
        "turnId": turn.turn_id,
        "runId": turn.run_id,
        "duplicate": duplicate,
    });
    if let Some(session_id) = &turn.session_id {
        body["sessionId"] = session_id.clone().into();
    }
    frame("run.accepted", body)
}

pub(super) fn mood_frame(mood: &MoodTransition, activity: &str) -> Outgoing {
    frame("mood", json!({ "mood": mood, "activity": activity }))
}

/// Her mood as it stands, while her persona is on.
async fn current_mood(db: &DatabaseConnection, owner: i32) -> Option<Outgoing> {
    if !merope::is_enabled().await {
        return None;
    }
    let state = merope::get_or_create_state(db, owner).await.ok()?;
    let affect = merope::store::affect_from_state(&state);
    let mood = MoodTransition::from_affect(
        &affect,
        &affect,
        "current",
        state.updated_at.timestamp_millis(),
    );
    Some(mood_frame(&mood, merope::current_activity(&state)))
}

/// Her mood as it changes outside a run (the appraisal of what they said
/// lands after the reply), for the site owner.
async fn follow_moods(
    tx: mpsc::Sender<Outgoing>,
    owner: i32,
    mut events: tokio::sync::broadcast::Receiver<NotificationEvent>,
) {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        match events.recv().await {
            Ok(NotificationEvent::MeropeStateChanged {
                user_id,
                mood,
                activity,
            }) if user_id == owner => {
                if tx.send(mood_frame(&mood, &activity)).await.is_err() {
                    return;
                }
            }
            Ok(_) | Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return,
        }
    }
}

/// How she acts on this event, when she still should: a directive while the
/// run is live or being caught up on, and a mood only as it happens.
pub(super) fn acting(
    turn: &Turn,
    sequence: u64,
    event: &AgentProgressEvent,
    live: bool,
) -> Option<Outgoing> {
    let directive = match event {
        AgentProgressEvent::PerformancePlan { performance } => {
            serde_json::to_value(performance).ok()
        }
        AgentProgressEvent::TaskCompleted { response, .. } => response
            .get("performance")
            .filter(|performance| !performance.is_null())
            .cloned(),
        AgentProgressEvent::MeropeStateChanged { mood, activity } if live => {
            return Some(mood_frame(mood, activity));
        }
        _ => None,
    }?;
    Some(frame(
        "perform",
        json!({
            "turnId": turn.turn_id,
            "runId": turn.run_id,
            "sequence": sequence,
            "directive": directive,
        }),
    ))
}

/// The turn's run events after `after_seq`, then live until it ends, then
/// `run.end`. A run already over when caught up on sends its record only:
/// she does not act it again.
async fn follow_run(tx: mpsc::Sender<Outgoing>, turn: Turn, run: Arc<AgentRun>, after_seq: u64) {
    let (_, sent_before, over_before) = run.snapshot().await;
    let envelopes = agent_run_envelopes(run);
    tokio::pin!(envelopes);
    let mut last = after_seq;
    while let Some(envelope) = envelopes.next().await {
        // Sequence 0 is a run_started made up for a run that had none.
        if (envelope.sequence == 0 && after_seq > 0)
            || (envelope.sequence > 0 && envelope.sequence <= after_seq)
        {
            continue;
        }
        let live = envelope.sequence > sent_before;
        let terminal = agent_run_event_is_terminal(&envelope.event);
        let acted = if live || !over_before {
            acting(&turn, envelope.sequence, &envelope.event, live)
        } else {
            None
        };
        let event = serde_json::to_value(&envelope.event).unwrap_or(Value::Null);
        let record = frame(
            "run.event",
            json!({
                "turnId": turn.turn_id,
                "runId": turn.run_id,
                "sequence": envelope.sequence,
                "event": event,
            }),
        );
        last = last.max(envelope.sequence);
        for outgoing in std::iter::once(record).chain(acted) {
            if tx.send(outgoing).await.is_err() {
                return;
            }
        }
        if terminal {
            break;
        }
    }
    let _ = tx
        .send(frame(
            "run.end",
            json!({
                "turnId": turn.turn_id,
                "runId": turn.run_id,
                "lastSequence": last,
            }),
        ))
        .await;
}
