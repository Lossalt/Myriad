//! One link, from the upgrade to the close: the handshake, what the child
//! sends, the runs it follows, her mood, and the heartbeat that also checks
//! the device is still one.

use std::collections::{HashMap, HashSet};
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

impl Outgoing {
    /// A frame that is not kept: an answer, or a latest value.
    pub(super) fn latest(t: &'static str, body: Value) -> Self {
        frame(t, body)
    }

    /// A reliable frame: kept until acked, by its id.
    pub(super) fn reliable(t: &'static str, id: String, body: Value) -> Self {
        Self {
            t,
            id: Some(id),
            body,
        }
    }
}

/// A frame as a child sends it. Every field but `id` is required.
#[derive(Deserialize)]
pub(super) struct Inbound {
    pub(super) t: String,
    pub(super) seq: u64,
    pub(super) ack: u64,
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
    let (registered, closed) = super::open(&device.id, device.user_id);
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
        registered,
        takes_say: false,
        playback: false,
        touch: false,
        worn: None,
        directions: HashMap::new(),
        taken: HashSet::new(),
        unacked: Vec::new(),
        said: HashSet::new(),
        senses: Default::default(),
        experience: false,
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
    /// This link in the registry.
    registered: u64,
    /// The child said it takes `say`.
    takes_say: bool,
    /// The child said it takes `playback`: her director's refinements of
    /// what it is saying, and reports of how far it got.
    playback: bool,
    /// The child said it sends `touch`.
    touch: bool,
    /// The set this app wears, as last told, once the child said it takes
    /// `wardrobe` (`Some(None)`: none it can wear).
    worn: Option<Option<super::outfit::Worn>>,
    /// The playback direction each followed run is being sent from.
    directions: HashMap<String, tokio::task::AbortHandle>,
    /// Ids of the child's reliable body frames already taken on this
    /// connection (`speech.delivered`, `touch`).
    taken: HashSet<String>,
    /// Reliable frames sent on this connection and not yet acked: (seq, id).
    unacked: Vec<(u64, String)>,
    /// Reliable ids sent on this connection.
    said: HashSet<String>,
    /// What the child said it senses for her.
    senses: super::senses::Senses,
    /// The child said it sends `experience` (and asks `work.recall`).
    experience: bool,
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
            self.unacked.retain(|(_, sent)| *sent != id);
            self.unacked.push((self.sent, id.clone()));
            self.said.insert(id.clone());
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
        self.acked(inbound.ack);
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
            "playback.observe" if self.playback => self.observe(inbound).await,
            "playback.end" if self.playback => self.playback_end(inbound).await,
            "speech.delivered" if self.playback => self.delivered(inbound).await,
            "touch.appraise" if self.touch => self.appraise(inbound).await,
            "touch" if self.touch => self.touched(inbound).await,
            "outfit.wear" if self.worn.is_some() => self.wear(inbound).await,
            "work.recall" if self.experience => self.recall(inbound),
            "experience" if self.experience => self.experienced(inbound).await,
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
            "capabilities": &capabilities,
            "turns": told,
            "serverTime": chrono::Utc::now().timestamp_millis(),
        });
        self.reply(frame("welcome", welcome));
        self.playback = capabilities
            .iter()
            .any(|capability| capability == "playback");
        self.touch = capabilities.iter().any(|capability| capability == "touch");
        self.senses = super::senses::Senses::of(&capabilities);
        // What it does for the owner comes up; what is written is told on its link.
        if capabilities
            .iter()
            .any(|capability| capability == "experience")
        {
            self.experience = true;
            super::takes_experience(&self.device.id, self.registered, self.tx.clone());
        }
        // Her lines for the app: what is still waiting goes again, after welcome.
        if capabilities.iter().any(|capability| capability == "say") {
            self.takes_say = true;
            super::takes_say(
                &self.device.id,
                self.registered,
                self.tx.clone(),
                self.playback,
            );
            for (id, body) in super::say::pending(&self.device.id) {
                self.reply(Outgoing::reliable("say", id, body));
            }
        }
        if let Some(mood) = current_mood(&self.db, self.owner).await {
            self.reply(mood);
        }
        // The set it wears: its own, or, the first time, the one it starts with.
        if capabilities
            .iter()
            .any(|capability| capability == "wardrobe")
        {
            let worn = super::outfit::put(&self.db, self.owner, &self.device.id, None)
                .await
                .ok()
                .flatten();
            if let Some(worn) = &worn {
                self.reply(super::outfit::frame(worn));
            }
            self.worn = Some(worn);
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
        // Saying something on the app is using her there.
        crate::services::agent::consciousness::native_used(self.owner, &self.device.id);
        let request = ProcessRequest {
            input: input.text,
            context: Some(ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: input.session_id,
                rig_state: input.rig_state,
                // Answered with what she senses there: the app's own.
                custom_data: crate::services::agent::consciousness::native_senses(
                    self.owner,
                    &self.device.id,
                ),
                on_device: Some(self.device.id.clone()),
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
        // Answered on the device: the site does not notify about it (the
        // run is so from its start, `on_device`).
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
            // Only what the child said it senses.
            let arrived = crate::services::agent::consciousness::remember_native_presence(
                self.owner,
                &self.device.id,
                &self.senses.admit(&inbound.body),
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
        self.directions.retain(|_, task| !task.is_finished());
        // Her director's refinements of the reply, once per run, while it is said.
        if self.playback && !self.directions.contains_key(&turn.run_id) {
            let task = tokio::spawn(super::body::follow_direction(
                self.tx.clone(),
                turn.clone(),
                run.clone(),
            ));
            self.directions
                .insert(turn.run_id.clone(), task.abort_handle());
        }
        let task = tokio::spawn(follow_run(self.tx.clone(), turn, run, after_seq));
        self.follows.insert(key, task.abort_handle());
    }

    /// The live run one of this device's turns started, for a body frame
    /// that names it; `None` (and the error sent) when it names another.
    async fn own_run(&mut self, run_id: &str, frame_seq: u64) -> Option<(Turn, Arc<AgentRun>)> {
        let Some(turn) = turns::by_run(&self.device.id, run_id) else {
            self.error("not_this_device", Some(frame_seq));
            return None;
        };
        // A run already gone has nothing left to direct.
        let run = get_live_run_for_user(run_id, self.owner).await?;
        Some((turn, run))
    }

    /// `playback.observe`: where saying the reply has got to, for her director.
    async fn observe(&mut self, inbound: Inbound) {
        let Ok(observed) = serde_json::from_value::<super::body::Observe>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if observed.upcoming_text.len() > super::body::UPCOMING_BYTES
            || observed.rig.to_string().len() > super::body::RIG_BYTES
        {
            self.error("too_large", Some(inbound.seq));
            return;
        }
        if let Some((_, run)) = self.own_run(&observed.run_id, inbound.seq).await {
            run.playback_direction.observe(
                crate::services::agent::playback_direction::PlaybackObservation {
                    upcoming_text: observed.upcoming_text,
                    rig: observed.rig,
                },
            );
        }
    }

    /// `playback.end`: the reply is said, or stopped being said.
    async fn playback_end(&mut self, inbound: Inbound) {
        let Ok(ended) = serde_json::from_value::<super::body::End>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if let Some((_, run)) = self.own_run(&ended.run_id, inbound.seq).await {
            run.playback_direction.close();
        }
    }

    /// `speech.delivered`: how much of her reply or line was said aloud. One
    /// cut off is marked in her history, so her next turn knows where she
    /// stopped.
    async fn delivered(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(delivered) = serde_json::from_value::<super::body::Delivered>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !self.taken.insert(id) {
            return;
        }
        let by = delivered.by().unwrap_or("unknown");
        if let Some(run_id) = &delivered.run_id {
            let Some(turn) = turns::by_run(&self.device.id, run_id) else {
                self.error("not_this_device", Some(seq));
                return;
            };
            if delivered.interrupted {
                let session = match turn.session_id.clone() {
                    Some(session) => Some(session),
                    None => get_run_for_user(run_id, self.owner)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|run| run.session_id().map(str::to_string)),
                };
                if let Some(session) = session {
                    let heard = delivered.spoken_chars.min(delivered.total_chars);
                    let (db, run_id) = (self.db.clone(), run_id.clone());
                    mark_cut_off(self.device.id.clone(), by, "reply", move || {
                        let (db, session, run_id) = (db.clone(), session.clone(), run_id.clone());
                        async move {
                            crate::services::agent::sessions::mark_spoken_reply_cut_off_at(
                                &db,
                                &session,
                                &run_id,
                                Some(heard),
                            )
                            .await
                        }
                    })
                    .await;
                }
            }
        } else if let Some(say_id) = delivered.say_id.clone() {
            if !valid_id(&say_id) {
                self.error("bad_id", Some(seq));
                return;
            }
            // A line of hers cut off reads as her replies do: maybe not
            // heard to the end, and how much was.
            if delivered.interrupted {
                let heard = delivered.spoken_chars.min(delivered.total_chars);
                let (db, owner) = (self.db.clone(), self.owner);
                mark_cut_off(self.device.id.clone(), by, "line", move || {
                    let (db, say_id) = (db.clone(), say_id.clone());
                    async move {
                        merope::store::mark_line_cut_off(&db, owner, &say_id, Some(heard)).await
                    }
                })
                .await;
            }
        }
    }

    /// `touch.appraise`: contact going on with her figure on the app; she
    /// answers with how she takes it, within the touch time.
    async fn appraise(&mut self, inbound: Inbound) {
        let Ok(appraise) = serde_json::from_value::<super::body::Appraise>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if !valid_id(&appraise.touch_id) {
            self.error("bad_id", Some(inbound.seq));
            return;
        }
        // Only a window on screen can be touched.
        let shown =
            crate::services::agent::consciousness::native_presence(self.owner, &self.device.id)
                .is_some_and(|live| live.page_visible);
        if !shown {
            self.reply(super::body::reaction(&appraise.touch_id, None));
            return;
        }
        let (db, owner, tx, seq) = (self.db.clone(), self.owner, self.tx.clone(), inbound.seq);
        tokio::spawn(async move {
            let answer = match crate::services::agent::merope::api::touch::appraise_ongoing(
                &db,
                owner,
                &appraise.touch,
            )
            .await
            {
                Ok(reaction) => super::body::reaction(&appraise.touch_id, reaction),
                Err(_) => frame("error", json!({ "code": "bad_frame", "ref": seq })),
            };
            let _ = tx.send(answer).await;
        });
    }

    /// `work.recall`: the Work memories a run on the app starts from, as the
    /// site's own Work recalls them.
    fn recall(&mut self, inbound: Inbound) {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Recall {
            request_id: String,
            query: String,
        }
        let Ok(recall) = serde_json::from_value::<Recall>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if !valid_id(&recall.request_id) {
            self.error("bad_id", Some(inbound.seq));
            return;
        }
        if recall.query.chars().count() > CONTRACT.limits.max_text_chars {
            self.error("too_large", Some(inbound.seq));
            return;
        }
        let (db, owner, tx) = (self.db.clone(), self.owner, self.tx.clone());
        tokio::spawn(async move {
            let memories = super::experience::recall(&db, owner, &recall.query).await;
            let _ = tx
                .send(super::experience::recalled_frame(
                    &recall.request_id,
                    &memories,
                ))
                .await;
        });
    }

    /// `experience`: something the app did for the owner, to become part of
    /// her. Taken once it is written; a resend of one already written is
    /// told so again.
    async fn experienced(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(experience) = serde_json::from_value::<super::experience::Experience>(inbound.body)
        else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !experience.readable() {
            self.error("bad_frame", Some(seq));
            return;
        }
        if super::experience::was_taken(&self.db, &self.device.id, &id).await {
            self.reply(super::experience::taken_frame(&id));
            return;
        }
        // Another copy is being written; it is told when that is done.
        if !super::experience::begin(&self.device.id, &id) {
            return;
        }
        tokio::spawn(super::experience::write(
            self.db.clone(),
            self.owner,
            self.device.id.clone(),
            id,
            experience,
        ));
    }

    /// `outfit.wear`: the owner put a set of her wardrobe on her on the app.
    async fn wear(&mut self, inbound: Inbound) {
        let Some(outfit_id) = inbound
            .body
            .get("outfitId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        match super::outfit::put(&self.db, self.owner, &self.device.id, Some(outfit_id)).await {
            Ok(Some(worn)) => {
                self.reply(super::outfit::frame(&worn));
                self.worn = Some(Some(worn));
            }
            Ok(None) => self.error("internal_error", Some(inbound.seq)),
            Err(_) => self.error("outfit_unknown", Some(inbound.seq)),
        }
    }

    /// `touch`: contact with her figure on the app has ended; she is told
    /// of it, and answers on the app.
    async fn touched(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(touched) = serde_json::from_value::<super::body::Touched>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !self.taken.insert(id) {
            return;
        }
        let live =
            crate::services::agent::consciousness::native_presence(self.owner, &self.device.id)
                .unwrap_or_default();
        if crate::services::agent::merope::api::touch::ended(
            self.owner,
            &touched.touch,
            &live,
            Some(&self.device.id),
        )
        .await
        .is_err()
        {
            self.error("bad_frame", Some(seq));
        }
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
        // What it wears changed on the host (its set's rig was made again,
        // or the set left her wardrobe).
        if let Some(told) = self.worn.clone() {
            let now = super::outfit::current(&self.db, &self.device.id).await;
            if now != told {
                if let Some(worn) = &now {
                    self.reply(super::outfit::frame(worn));
                }
                self.worn = Some(now);
            }
        }
        if self.takes_say {
            // Lines that could not be queued when said, and none that expired.
            let pending = super::say::pending(&self.device.id);
            self.unacked
                .retain(|(_, id)| pending.iter().any(|(waiting, _)| waiting == id));
            for (id, body) in pending {
                if !self.said.contains(&id) {
                    self.reply(Outgoing::reliable("say", id, body));
                }
            }
        }
        None
    }

    /// The child's ack covers everything up to `ack` on this connection.
    fn acked(&mut self, ack: u64) {
        let device = &self.device.id;
        self.unacked.retain(|(seq, id)| {
            if *seq <= ack {
                super::say::acked(device, id);
                false
            } else {
                true
            }
        });
    }

    /// The link is closed: stop sending, and the person is not there now
    /// (unless `gone` is false: another link of the device took over).
    fn finish(&mut self, gone: bool) {
        for (_, task) in self.follows.drain().chain(self.directions.drain()) {
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

/// Marks what the app cut off in her history once it is kept there: a reply
/// is saved whole when its run ends and a line of hers once it was
/// delivered, and either can be stopped aloud before that. Tries now, then
/// every half second for a minute.
async fn mark_cut_off<F, Fut>(device: String, by: &str, what: &'static str, mark: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<bool, sea_orm::DbErr>> + Send,
{
    match mark().await {
        Ok(true) => tracing::info!(device = %device, by, what, "[link] cut off on the app"),
        Ok(false) => {
            let by = by.to_string();
            tokio::spawn(async move {
                for _ in 0..120 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    match mark().await {
                        Ok(false) => continue,
                        Ok(true) => {
                            tracing::info!(device = %device, by, what, "[link] cut off on the app")
                        }
                        Err(error) => {
                            tracing::warn!(%error, device = %device, what, "[link] could not mark a cut-off")
                        }
                    }
                    return;
                }
            });
        }
        Err(error) => {
            tracing::warn!(%error, device = %device, what, "[link] could not mark a cut-off")
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
