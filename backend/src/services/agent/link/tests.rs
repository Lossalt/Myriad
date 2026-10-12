//! Her link against its shared contract and fixtures, and over a real
//! WebSocket against the agent router on a real database (an isolated
//! schema; the database tests are skipped without a test database).

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest};

use super::connection::{Hello, Inbound, Input, Resume, accepted, acting, mood_frame};
use super::contract::{
    CHILD_FRAMES, CONTRACT, CONTRACT_JSON, Close, HOST_ERROR_CODES, HOST_FRAMES, Timing, valid_id,
};
use super::say::SayKind;
use super::turns::{self, Turn};
use crate::services::agent::AgentProgressEvent;
use crate::services::agent::merope::{MoodTransition, PerformanceDirective};
use crate::services::persona_sync::devices::{self, Device, RevokeReason};

const FIXTURES_JSON: &str = include_str!("../../../../../shared/her_link_fixtures.json");

fn fixtures() -> Vec<Value> {
    let fixtures: Value = serde_json::from_str(FIXTURES_JSON).expect("her_link_fixtures.json");
    fixtures["frames"].as_array().expect("frames").clone()
}

fn fixture(t: &str) -> Value {
    fixtures()
        .into_iter()
        .find(|frame| frame["t"] == t)
        .unwrap_or_else(|| panic!("no fixture for {t}"))
}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn strings(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn the_host_speaks_the_contract() {
    let raw: Value = serde_json::from_str(CONTRACT_JSON).unwrap();
    assert_eq!(CONTRACT.protocol, 1);
    assert_eq!(CONTRACT.path, "/api/agent/link");
    assert_eq!(raw["auth"]["tokenPrefix"], devices::TOKEN_PREFIX);
    assert_eq!(CONTRACT.hosts, ["site"]);
    assert_eq!(CONTRACT.surfaces, ["app"]);
    // Every frame type is one the host sends, takes, or both.
    let declared: BTreeSet<String> = CONTRACT.frames.keys().cloned().collect();
    let known: BTreeSet<String> = HOST_FRAMES
        .iter()
        .chain(CHILD_FRAMES.iter())
        .map(|t| t.to_string())
        .collect();
    assert_eq!(declared, known);
    for (t, spec) in &CONTRACT.frames {
        let host = HOST_FRAMES.contains(&t.as_str());
        let child = CHILD_FRAMES.contains(&t.as_str());
        let from = match (host, child) {
            (true, true) => "either",
            (true, false) => "host",
            _ => "child",
        };
        assert_eq!(spec.from, from, "{t}");
        assert!(
            raw["deliveries"].get(&spec.delivery).is_some(),
            "{t}: {}",
            spec.delivery
        );
        for field in &spec.optional {
            assert!(!spec.required.contains(field), "{t}.{field}");
        }
    }
    assert_eq!(CONTRACT.frames["input"].delivery, "reliable");
    assert_eq!(CONTRACT.frames["say"].delivery, "reliable");
    assert!(CONTRACT.capabilities.iter().any(|c| c == "say"));
    // Her body on the app: what it says up is kept until taken.
    for t in ["speech.delivered", "touch", "experience"] {
        assert_eq!(CONTRACT.frames[t].delivery, "reliable", "{t}");
    }
    assert!(CONTRACT.capabilities.iter().any(|c| c == "experience"));
    assert_eq!(
        super::experience::RECALLED,
        4,
        "as many as the site's own Work recalls"
    );
    for capability in ["playback", "touch"] {
        assert!(CONTRACT.capabilities.iter().any(|c| c == capability));
    }
    assert_eq!(
        CONTRACT.timing.touch_appraise_ms, 3000,
        "as the site's own touch"
    );
    let kinds: Vec<&str> = [SayKind::Greeting, SayKind::Proactive, SayKind::FaceToFace]
        .iter()
        .map(|kind| kind.as_str())
        .collect();
    assert_eq!(CONTRACT.say_kinds, kinds);
    assert_eq!(
        Duration::from_millis(CONTRACT.timing.say_ttl_ms),
        Duration::from_secs(crate::services::agent::presence_window::PRESENCE_WINDOW_SECS as u64),
        "a line waits as long as presence lasts"
    );
    // Codes: what the host sends is declared, and declared codes are what it sends.
    let declared: BTreeSet<String> = CONTRACT.error_codes.keys().cloned().collect();
    assert_eq!(declared, strings(&HOST_ERROR_CODES));
    let mut numbers = BTreeSet::new();
    for spec in CONTRACT.close_codes.values() {
        assert!((4000..5000).contains(&spec.code));
        assert!(numbers.insert(spec.code), "close code {} twice", spec.code);
    }
    for close in Close::ALL {
        assert!(CONTRACT.close_codes.contains_key(close.name()), "{close:?}");
    }
    for name in ["revoked", "owner_changed"] {
        assert!(CONTRACT.close_codes[name].forget_credentials);
        assert!(!CONTRACT.close_codes[name].reconnect);
    }
    for name in ["timeout", "unavailable", "bad_frame"] {
        assert!(!CONTRACT.close_codes[name].forget_credentials);
        assert!(CONTRACT.close_codes[name].reconnect);
    }
    // Limits are the run's own, and the id pattern is what valid_id checks.
    assert_eq!(
        CONTRACT.limits.max_text_chars,
        crate::services::agent::run::MAX_INPUT_LEN
    );
    assert_eq!(CONTRACT.limits.id_pattern, "^[A-Za-z0-9_.:-]{1,64}$");
    for id in ["t_1", "A.b:c-D_9", &"x".repeat(64)] {
        assert!(valid_id(id), "{id}");
    }
    for id in ["", "has space", "turn/1", "表", &"x".repeat(65)] {
        assert!(!valid_id(id), "{id}");
    }
    // The heartbeat is the SSE keep-alive's.
    assert_eq!(Timing::from_contract().heartbeat, Duration::from_secs(15));
    assert_eq!(Timing::from_contract().silence(), Duration::from_secs(30));
    assert!(CONTRACT.limits.max_frame_bytes >= CONTRACT.limits.max_text_chars * 4);
}

#[test]
fn every_frame_has_a_fixture_that_fits_it() {
    let frames = fixtures();
    let envelope_required: BTreeSet<String> = CONTRACT.envelope.required.iter().cloned().collect();
    let envelope_all: BTreeSet<String> = envelope_required
        .iter()
        .chain(CONTRACT.envelope.optional.iter())
        .cloned()
        .collect();
    let mut seen = BTreeSet::new();
    for frame in &frames {
        let t = frame["t"].as_str().expect("t");
        let spec = CONTRACT
            .frames
            .get(t)
            .unwrap_or_else(|| panic!("{t} is not in the contract"));
        seen.insert(t.to_string());
        let envelope = keys(frame);
        assert!(
            envelope.is_superset(&envelope_required),
            "{t}: {envelope:?}"
        );
        assert!(envelope.is_subset(&envelope_all), "{t}: {envelope:?}");
        assert!(frame["seq"].as_u64().is_some_and(|seq| seq >= 1), "{t}");
        assert!(frame["ack"].as_u64().is_some(), "{t}");
        assert!(frame["at"].as_i64().is_some(), "{t}");
        assert_eq!(
            frame.get("id").is_some(),
            spec.delivery == "reliable",
            "{t}"
        );
        let body = keys(&frame["body"]);
        let required: BTreeSet<String> = spec.required.iter().cloned().collect();
        let all: BTreeSet<String> = required
            .iter()
            .chain(spec.optional.iter())
            .cloned()
            .collect();
        assert!(body.is_superset(&required), "{t}: {body:?}");
        assert!(body.is_subset(&all), "{t}: {body:?}");
        // What a child sends, the host reads.
        if spec.from != "host" {
            let inbound: Inbound = serde_json::from_value(frame.clone()).expect(t);
            match t {
                "hello" => {
                    let hello: Hello = serde_json::from_value(inbound.body).unwrap();
                    assert_eq!((hello.protocol, hello.role.as_str()), (1, "child"));
                    assert!(CONTRACT.surfaces.contains(&hello.surface));
                    assert!(
                        hello
                            .capabilities
                            .iter()
                            .all(|c| CONTRACT.capabilities.contains(c))
                    );
                }
                "input" => {
                    let input: Input = serde_json::from_value(inbound.body).unwrap();
                    assert!(valid_id(inbound.id.as_deref().unwrap()));
                    assert!(!input.text.is_empty() && CONTRACT.surfaces.contains(&input.surface));
                    assert!(input.session_id.is_some());
                    assert!(myriad_merope::sanitize_rig_state(&input.rig_state.unwrap()).is_some());
                }
                "run.resume" => {
                    let resume: Resume = serde_json::from_value(inbound.body).unwrap();
                    assert!(valid_id(&resume.turn_id) && resume.run_id.is_some());
                }
                "presence" => {
                    let live =
                        crate::services::agent::consciousness::live_presence_from_custom_data(
                            &inbound.body,
                        );
                    assert!(live.page_visible && live.panel_visible);
                    assert_eq!(live.perception_payload.len(), 1);
                    assert!(live.music_status.is_some());
                }
                "playback.observe" => {
                    let observed: super::body::Observe =
                        serde_json::from_value(inbound.body).unwrap();
                    assert!(observed.upcoming_text.len() <= super::body::UPCOMING_BYTES);
                    assert!(myriad_merope::sanitize_rig_state(&observed.rig).is_some());
                }
                "playback.end" => {
                    let _: super::body::End = serde_json::from_value(inbound.body).unwrap();
                }
                "speech.delivered" => {
                    assert!(inbound.id.as_deref().is_some_and(valid_id));
                    let delivered: super::body::Delivered =
                        serde_json::from_value(inbound.body).unwrap();
                    assert!(delivered.interrupted && delivered.by() == Some("stop"));
                    assert!(delivered.spoken_chars <= delivered.total_chars);
                }
                "touch.appraise" => {
                    let appraise: super::body::Appraise =
                        serde_json::from_value(inbound.body).unwrap();
                    assert!(valid_id(&appraise.touch_id));
                }
                "touch" => {
                    assert!(inbound.id.as_deref().is_some_and(valid_id));
                    let _: super::body::Touched = serde_json::from_value(inbound.body).unwrap();
                }
                "experience" => {
                    assert!(inbound.id.as_deref().is_some_and(valid_id));
                    let experience: super::experience::Experience =
                        serde_json::from_value(inbound.body).unwrap();
                    assert!(experience.readable());
                    let run = experience.run(OWNER, "dev_1");
                    assert!(run.success && run.origin.as_deref() == Some("dev_1"));
                    assert_eq!(run.step_outputs.len(), 1);
                    assert_eq!(run.capabilities, ["time.info"]);
                }
                "work.recall" => {
                    assert!(inbound.body["requestId"].as_str().is_some_and(valid_id));
                }
                _ => {}
            }
        }
    }
    let declared: BTreeSet<String> = CONTRACT.frames.keys().cloned().collect();
    assert_eq!(seen, declared, "one fixture per frame");

    // The directive and the mood are the shared shapes, byte for byte.
    let directive = fixture("perform")["body"]["directive"].clone();
    let parsed: PerformanceDirective = serde_json::from_value(directive.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), directive);
    for cue in &parsed.plan.cues {
        assert!(myriad_merope::PERFORMANCE_CUE_INTENTS.contains(&cue.intent.as_str()));
        assert!(myriad_merope::PERFORMANCE_INTERRUPT_MODES.contains(&cue.interrupt.as_str()));
    }
    let mood = fixture("mood")["body"]["mood"].clone();
    let parsed: MoodTransition = serde_json::from_value(mood.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), mood);

    // What the host sends has the fixtures' shape.
    let turn = Turn {
        turn_id: "t_02".into(),
        run_id: "run_fedcba9876543210fedcba9876543210".into(),
        session_id: Some("sess_1".into()),
    };
    let sent = accepted(&turn, false);
    assert_eq!(sent.t, "run.accepted");
    assert_eq!(sent.body, fixture("run.accepted")["body"]);
    let performance: PerformanceDirective =
        serde_json::from_value(fixture("perform")["body"]["directive"].clone()).unwrap();
    let event = AgentProgressEvent::PerformancePlan { performance };
    let sent = acting(&turn, 3, &event, true).unwrap();
    assert_eq!(sent.t, "perform");
    assert_eq!(sent.body, fixture("perform")["body"]);
    let mood: MoodTransition = serde_json::from_value(mood).unwrap();
    let sent = mood_frame(&mood, "talking");
    assert_eq!(sent.body, fixture("mood")["body"]);
    let say = fixture("say");
    let built = super::say::body(
        &super::say::Say {
            id: say["id"].as_str().unwrap().into(),
            kind: SayKind::Greeting,
            text: say["body"]["text"].as_str().unwrap().into(),
            session_id: Some("sess_1".into()),
            perform: Some(say["body"]["perform"].clone()),
            event_key: Some("agent.merope.greeting".into()),
        },
        say["body"]["saidAt"].as_i64().unwrap(),
    );
    assert_eq!(built, say["body"]);
    // Her director's refinement while the app says the reply.
    let live = fixtures()
        .into_iter()
        .find(|frame| frame["t"] == "perform" && frame["body"].get("live").is_some())
        .expect("a live perform fixture");
    let sent = super::body::live_perform(
        &turn,
        live["body"]["live"].as_u64().unwrap(),
        live["body"]["directive"].clone(),
    );
    assert_eq!(sent.t, "perform");
    assert_eq!(sent.body, live["body"]);
    let _: PerformanceDirective =
        serde_json::from_value(live["body"]["directive"].clone()).unwrap();
    let sent = super::body::reaction("touch_01", Some(json!({ "reaction": "accept" })));
    assert_eq!(sent.body, fixture("touch.reaction")["body"]);
    assert_eq!(
        super::body::reaction("touch_01", None).body["reaction"],
        Value::Null
    );
    let _: PerformanceDirective = serde_json::from_value(say["body"]["perform"].clone()).unwrap();
    // What a child's experience is answered with.
    let recalled = fixture("work.recalled");
    let memories: Vec<String> =
        serde_json::from_value(recalled["body"]["memories"].clone()).unwrap();
    let sent = super::experience::recalled_frame("wr_01", &memories);
    assert_eq!((sent.t, &sent.body), ("work.recalled", &recalled["body"]));
    let taken = fixture("experience.taken");
    let sent = super::experience::taken_frame(taken["body"]["id"].as_str().unwrap());
    assert_eq!((sent.t, &sent.body), ("experience.taken", &taken["body"]));
    assert_eq!(taken["body"]["id"], fixture("experience")["id"]);
    // A mood that comes with a run counts only as it happens.
    let event = AgentProgressEvent::MeropeStateChanged {
        mood,
        activity: "talking".into(),
    };
    assert_eq!(acting(&turn, 4, &event, true).unwrap().t, "mood");
    assert!(acting(&turn, 4, &event, false).is_none());
    // The reply's own directive, at the end of a run.
    let event = AgentProgressEvent::TaskCompleted {
        task_id: "task".into(),
        success: true,
        response: Box::new(json!({ "success": true, "performance": directive })),
    };
    assert_eq!(
        acting(&turn, 9, &event, false).unwrap().body["directive"],
        directive
    );
    let event = AgentProgressEvent::TaskCompleted {
        task_id: "task".into(),
        success: true,
        response: Box::new(json!({ "success": true, "performance": null })),
    };
    assert!(acting(&turn, 9, &event, true).is_none());
}

// --- Over a real WebSocket ------------------------------------------------

const OWNER: i32 = 41;
const WAIT: Duration = Duration::from_secs(20);

struct Site {
    address: SocketAddr,
    db: DatabaseConnection,
    fixture: crate::federation::test_db::SchemaDb,
    server: tokio::task::JoinHandle<()>,
}

impl Site {
    /// Stops serving and drops the schema.
    async fn close(self) {
        self.server.abort();
        self.fixture.close().await;
    }
}

/// The agent router on an isolated schema, with the site's owner.
async fn site() -> Option<Site> {
    let fixture = crate::federation::test_db::SchemaDb::new_or_media().await?;
    let db = fixture.db.clone();
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, username, is_admin, is_owner) VALUES ({OWNER}, 'owner{OWNER}', true, true)"
    ))
    .await
    .unwrap();
    crate::services::principal::invalidate_site_owner_cache();
    let state = crate::state::AppState::new(
        db.clone(),
        crate::config::AppConfig::default(),
        crate::config::DynamicConfig::default(),
    );
    let app = axum::Router::new()
        .nest(
            "/api/agent",
            crate::api::agent::create_agent_routes(state.clone()),
        )
        .with_state(state);
    Some(serve_app(db, fixture, app).await)
}

async fn serve_app(
    db: DatabaseConnection,
    fixture: crate::federation::test_db::SchemaDb,
    app: axum::Router,
) -> Site {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Site {
        address,
        db,
        fixture,
        server,
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn dial(site: &Site, token: Option<&str>) -> Result<Socket, tungstenite::Error> {
    let mut request = format!("ws://{}/api/agent/link", site.address)
        .into_client_request()
        .unwrap();
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

/// A refused dial: the HTTP status and the error code it answered with.
async fn refused(site: &Site, token: Option<&str>) -> (u16, Value) {
    match dial(site, token).await {
        Err(tungstenite::Error::Http(response)) => {
            let body = response
                .body()
                .as_ref()
                .and_then(|body| serde_json::from_slice(body).ok())
                .unwrap_or(Value::Null);
            (response.status().as_u16(), body)
        }
        Err(error) => panic!("refused with {error}"),
        Ok(_) => panic!("the link opened"),
    }
}

/// The child end of a link, numbering what it sends like a child must.
struct Child {
    socket: Socket,
    seq: u64,
    heard: u64,
}

impl Child {
    async fn open(site: &Site, token: &str) -> Self {
        Self {
            socket: dial(site, Some(token)).await.expect("the link opens"),
            seq: 0,
            heard: 0,
        }
    }

    async fn send(&mut self, t: &str, id: Option<&str>, body: Value) -> u64 {
        self.seq += 1;
        let mut frame = json!({
            "t": t, "seq": self.seq, "ack": self.heard,
            "at": chrono::Utc::now().timestamp_millis(), "body": body,
        });
        if let Some(id) = id {
            frame["id"] = id.into();
        }
        self.socket
            .send(Message::Text(frame.to_string().into()))
            .await
            .unwrap();
        self.seq
    }

    async fn raw(&mut self, message: Message) {
        self.socket.send(message).await.unwrap();
    }

    /// The next frame; the host's own pings are left out.
    async fn next(&mut self) -> Value {
        let by = tokio::time::Instant::now() + WAIT;
        loop {
            let message = tokio::time::timeout_at(by, self.socket.next())
                .await
                .expect("a frame in time")
                .expect("the link is open")
                .expect("a frame");
            let Message::Text(text) = message else {
                if let Message::Close(close) = message {
                    panic!("closed: {close:?}");
                }
                continue;
            };
            let frame: Value = serde_json::from_str(text.as_str()).unwrap();
            let seq = frame["seq"].as_u64().unwrap();
            assert_eq!(seq, self.heard + 1, "the host numbers its frames: {frame}");
            self.heard = seq;
            // A child answers the host's pings, or the host lets it go.
            if frame["t"] == "ping" {
                self.send("pong", None, json!({})).await;
                continue;
            }
            return frame;
        }
    }

    async fn expect(&mut self, t: &str) -> Value {
        let frame = self.next().await;
        assert_eq!(frame["t"], t, "{frame}");
        frame
    }

    /// The close the host sends: its code and reason.
    async fn closed(&mut self) -> (u16, String) {
        loop {
            let message = tokio::time::timeout(WAIT, self.socket.next())
                .await
                .expect("a close in time");
            match message {
                Some(Ok(Message::Close(Some(close)))) => {
                    return (u16::from(close.code), close.reason.to_string());
                }
                Some(Ok(Message::Close(None))) | None | Some(Err(_)) => {
                    panic!("closed without a reason")
                }
                Some(Ok(_)) => {}
            }
        }
    }

    async fn hello(&mut self) -> Value {
        self.hello_with(&["rig", "telepathy", "rig", "notify"])
            .await
    }

    async fn hello_with(&mut self, capabilities: &[&str]) -> Value {
        self.send(
            "hello",
            None,
            json!({ "protocol": 1, "role": "child", "surface": "app",
                    "capabilities": capabilities }),
        )
        .await;
        self.expect("welcome").await
    }
}

async fn device(db: &DatabaseConnection, name: &str) -> (Device, String) {
    devices::add(db, OWNER, name).await.unwrap()
}

#[tokio::test]
async fn the_link_takes_only_a_live_device_token() {
    let Some(site) = site().await else {
        return;
    };
    let (_, token) = device(&site.db, "Mac").await;
    let (status, body) = refused(&site, None).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (401, Some("device_token_required"))
    );
    let forged = format!("mys_{}", "A".repeat(43));
    let (status, body) = refused(&site, Some(&forged)).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (401, Some("device_token_invalid"))
    );
    // A session's bearer token is no device token.
    let (status, body) = refused(&site, Some("eyJhbGciOiJIUzI1NiJ9.e30.x")).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (401, Some("device_token_invalid"))
    );
    let (removed, removed_token) = device(&site.db, "Old Mac").await;
    devices::revoke(&site.db, OWNER, &removed.id, RevokeReason::ByOwner)
        .await
        .unwrap();
    let (status, body) = refused(&site, Some(&removed_token)).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (401, Some("device_revoked"))
    );
    assert!(!body.to_string().contains(&removed_token));
    // A device of a previous owner is refused and removed.
    site.db
        .execute_unprepared(&format!(
            "INSERT INTO users (id, username) VALUES ({0}, 'user{0}')",
            OWNER + 1
        ))
        .await
        .unwrap();
    let (stranger, stranger_token) = devices::add(&site.db, OWNER + 1, "Theirs").await.unwrap();
    let (status, body) = refused(&site, Some(&stranger_token)).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (403, Some("owner_changed"))
    );
    let listed = devices::list(&site.db, OWNER + 1).await.unwrap();
    assert!(
        listed
            .iter()
            .any(|d| d.id == stranger.id && d.revoked_at.is_some())
    );
    // The live one gets in.
    let mut child = Child::open(&site, &token).await;
    child.hello().await;
    drop(child);
    site.close().await;
}

#[tokio::test]
async fn hello_welcome_and_what_the_link_will_not_take() {
    let Some(site) = site().await else {
        return;
    };
    let (mac, token) = device(&site.db, "MacBook").await;
    let mut child = Child::open(&site, &token).await;
    // Nothing before hello.
    let presence = child
        .send(
            "presence",
            None,
            json!({ "presence": { "pageVisible": true } }),
        )
        .await;
    let error = child.expect("error").await;
    assert_eq!(
        error["body"],
        json!({ "code": "not_ready", "ref": presence })
    );
    let welcome = child.hello().await;
    assert_eq!(welcome["ack"], 2, "the host acknowledges what it heard");
    let body = &welcome["body"];
    assert_eq!(body["protocol"], 1);
    assert_eq!(
        (body["role"].as_str(), body["host"].as_str()),
        (Some("host"), Some("site"))
    );
    assert_eq!(body["deviceId"], mac.id);
    assert_eq!(body["deviceName"], "MacBook");
    assert_eq!(body["heartbeatMs"], 15000);
    assert_eq!(body["capabilities"], json!(["rig", "notify"]));
    assert_eq!(body["turns"], json!([]));
    assert!(body["lineage"].is_null() && body["hostEpoch"].is_null());
    assert!(body["serverTime"].as_i64().unwrap() > 0);
    assert_eq!(keys(&welcome), keys(&fixture("welcome")));
    assert_eq!(keys(body), keys(&fixture("welcome")["body"]));
    // Her persona is off on this site: no mood follows the welcome.
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;
    // What it does not know, it says so and stays up.
    let teleport = child.send("teleport", None, json!({})).await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "unknown_frame", "ref": teleport })
    );
    let backwards = child.send("welcome", None, json!({})).await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_frame", "ref": backwards })
    );
    let shapeless = child
        .send("presence", None, json!({ "visible": true }))
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_frame", "ref": shapeless })
    );
    child
        .send("presence", None, fixture("presence")["body"].clone())
        .await;
    child.raw(Message::Text("not json".into())).await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_frame" })
    );
    child.raw(Message::Binary(vec![1, 2, 3].into())).await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "binary_unsupported" })
    );
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;

    // A handshake that is not one closes the link, saying why.
    for (hello, close) in [
        (
            json!({ "protocol": 1, "role": "host", "surface": "app" }),
            Close::HostIsSite,
        ),
        (
            json!({ "protocol": 2, "role": "child", "surface": "app" }),
            Close::UnsupportedProtocol,
        ),
        (
            json!({ "protocol": 1, "role": "child", "surface": "fridge" }),
            Close::BadFrame,
        ),
        (json!({ "role": "child" }), Close::BadFrame),
    ] {
        let mut other = Child::open(&site, &token).await;
        other.send("hello", None, hello).await;
        assert_eq!(
            other.closed().await,
            (close.code(), close.name().to_string())
        );
    }
    let mut other = Child::open(&site, &token).await;
    other.raw(Message::Text("{".into())).await;
    assert_eq!(other.closed().await, (4400, "bad_frame".to_string()));
    // Opening those replaced the first link: one link per device.
    assert_eq!(child.closed().await, (4003, "replaced".to_string()));

    // Removing the device closes its link at once, and it cannot come back.
    let mut child = Child::open(&site, &token).await;
    child.hello().await;
    devices::revoke(&site.db, OWNER, &mac.id, RevokeReason::ByOwner)
        .await
        .unwrap();
    assert_eq!(child.closed().await, (4001, "revoked".to_string()));
    let (status, body) = refused(&site, Some(&token)).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (401, Some("device_revoked"))
    );
    site.close().await;
}

/// The timings, shortened: no hello in time, and a child gone quiet.
#[tokio::test]
async fn a_quiet_child_is_let_go() {
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = fixture.db.clone();
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, username, is_admin, is_owner) VALUES ({OWNER}, 'owner{OWNER}', true, true)"
    ))
    .await
    .unwrap();
    crate::services::principal::invalidate_site_owner_cache();
    let (mac, token) = device(&db, "Mac").await;
    let timing = Timing {
        heartbeat: Duration::from_millis(100),
        missed: 2,
        hello: Duration::from_millis(300),
    };
    let link_db = db.clone();
    let app = axum::Router::new().route(
        "/api/agent/link",
        axum::routing::get(move |upgrade: axum::extract::ws::WebSocketUpgrade| {
            let (db, device) = (link_db.clone(), mac.clone());
            async move {
                upgrade.on_upgrade(move |socket| {
                    super::connection::serve_with(socket, db, device, timing)
                })
            }
        }),
    );
    let site = serve_app(db, fixture, app).await;
    let mut child = Child::open(&site, &token).await;
    assert_eq!(child.closed().await, (4408, "timeout".to_string()));
    // After hello the host pings; a child that never answers is gone.
    let mut child = Child::open(&site, &token).await;
    let welcome = child.hello().await;
    assert_eq!(welcome["body"]["heartbeatMs"], 100);
    assert_eq!(child.closed().await, (4408, "timeout".to_string()));
    site.close().await;
}

#[tokio::test]
async fn an_input_starts_a_run_for_the_owner_and_streams_it() {
    let Some(site) = site().await else {
        return;
    };
    let (_, token) = device(&site.db, "Mac").await;
    let mut child = Child::open(&site, &token).await;
    child.hello().await;
    let sent = child
        .send(
            "input",
            Some("t_hi"),
            json!({ "surface": "app", "text": "你好", "rigState": { "expression": "warm" } }),
        )
        .await;
    let accepted = child.expect("run.accepted").await;
    assert!(accepted["ack"].as_u64().unwrap() >= sent);
    let body = &accepted["body"];
    assert_eq!(
        (body["turnId"].as_str(), body["duplicate"].as_bool()),
        (Some("t_hi"), Some(false))
    );
    let run_id = body["runId"].as_str().unwrap().to_string();
    assert!(body["sessionId"].as_str().is_some_and(|id| !id.is_empty()));
    // The run as it goes, in its own sequence. This site has no model to
    // answer with, so once it is under way its owner stops it (how a run's
    // end reaches the child is the resume test's).
    let mut sequences = Vec::new();
    while sequences.len() < 3 {
        let frame = child.next().await;
        match frame["t"].as_str().unwrap() {
            "run.event" => {
                assert_eq!(frame["body"]["turnId"], "t_hi");
                assert_eq!(frame["body"]["runId"], run_id.as_str());
                if sequences.is_empty() {
                    assert_eq!(frame["body"]["event"]["type"], "run_started");
                    assert_eq!(frame["body"]["event"]["runId"], run_id.as_str());
                }
                sequences.push(frame["body"]["sequence"].as_u64().unwrap());
            }
            "perform" | "mood" => {}
            other => panic!("{other}: {frame}"),
        }
    }
    assert_eq!(sequences, [1, 2, 3]);
    let run = crate::services::agent::run_hub::get_live_run_for_user(&run_id, OWNER)
        .await
        .expect("the owner's run");
    // Answered on the app: nothing of it reached the site's notifications,
    // its start included (D26).
    assert!(run.site_notices().is_empty(), "{:?}", run.site_notices());
    run.abort_execution().await;
    // The run is the site owner's, and their message is kept.
    assert!(
        crate::services::agent::run_hub::get_live_run_for_user(&run_id, OWNER)
            .await
            .is_some()
    );
    let kept = site
        .db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "SELECT COUNT(*)::BIGINT AS n FROM agent_messages m JOIN agent_sessions s \
                 ON s.id = m.session_id WHERE s.user_id = {OWNER} AND m.role = 'user' AND m.content = '你好'"
            ),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(kept, 1);

    // Sent again (no ack seen), it is the same turn: no second run.
    child
        .send(
            "input",
            Some("t_hi"),
            json!({ "surface": "app", "text": "你好" }),
        )
        .await;
    let again = child.expect("run.accepted").await;
    assert_eq!(again["body"]["duplicate"], true);
    assert_eq!(again["body"]["runId"], run_id.as_str());
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;

    // Turns the host does not start.
    for (id, body, code) in [
        (
            "t_web",
            json!({ "surface": "web", "text": "hi" }),
            "surface_unsupported",
        ),
        (
            "t_blank",
            json!({ "surface": "app", "text": "  " }),
            "empty_input",
        ),
        (
            "t_long",
            json!({ "surface": "app", "text": "字".repeat(CONTRACT.limits.max_text_chars + 1) }),
            "input_too_long",
        ),
    ] {
        child.send("input", Some(id), body).await;
        let rejected = child.expect("run.rejected").await;
        assert_eq!(rejected["body"], json!({ "turnId": id, "code": code }));
    }
    let spaced = child
        .send(
            "input",
            Some("t hi"),
            json!({ "surface": "app", "text": "hi" }),
        )
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_id", "ref": spaced })
    );
    let unnamed = child
        .send("input", None, json!({ "surface": "app", "text": "hi" }))
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_frame", "ref": unnamed })
    );
    drop(child);
    site.close().await;
}

#[tokio::test]
async fn performing_and_resuming_after_a_reconnect() {
    let Some(site) = site().await else {
        return;
    };
    let (mac, token) = device(&site.db, "Mac").await;
    let run = crate::services::agent::run_hub::create_live_run_for_test(OWNER).await;
    let run_id = run.run_id().to_string();
    turns::remember(
        &site.db,
        &mac.id,
        OWNER,
        Turn {
            turn_id: "t_p".into(),
            run_id: run_id.clone(),
            session_id: None,
        },
    )
    .await;
    let token_event = |token: &str| AgentProgressEvent::SummaryToken {
        token: token.into(),
        done: false,
    };
    let directive: PerformanceDirective =
        serde_json::from_value(fixture("perform")["body"]["directive"].clone()).unwrap();

    let mut child = Child::open(&site, &token).await;
    let welcome = child.hello().await;
    assert_eq!(
        welcome["body"]["turns"],
        json!([{ "turnId": "t_p", "runId": run_id, "lastSequence": 1, "completed": false }])
    );
    child
        .send(
            "run.resume",
            None,
            json!({ "turnId": "t_p", "afterSeq": 0 }),
        )
        .await;
    let started = child.expect("run.event").await;
    assert_eq!(started["body"]["sequence"], 1);
    assert_eq!(started["body"]["event"]["type"], "run_started");
    run.publish(AgentProgressEvent::PerformancePlan {
        performance: directive.clone(),
    })
    .await;
    let record = child.expect("run.event").await;
    assert_eq!(record["body"]["event"]["type"], "performance_plan");
    // The perform frame is the fixture's, field for field.
    let perform = child.expect("perform").await;
    let expected = fixture("perform");
    assert_eq!(keys(&perform), keys(&expected));
    assert_eq!(keys(&perform["body"]), keys(&expected["body"]));
    assert_eq!(perform["body"]["directive"], expected["body"]["directive"]);
    assert_eq!(perform["body"]["sequence"], 2);
    assert_eq!(perform["body"]["runId"], run_id.as_str());
    run.publish(token_event("还")).await;
    assert_eq!(child.expect("run.event").await["body"]["sequence"], 3);

    // The link drops; the run goes on.
    drop(child);
    run.publish(token_event("不错")).await;
    run.publish(AgentProgressEvent::PerformancePlan {
        performance: directive,
    })
    .await;

    let mut child = Child::open(&site, &token).await;
    let welcome = child.hello().await;
    assert_eq!(welcome["body"]["turns"][0]["lastSequence"], 5);
    assert_eq!(welcome["body"]["turns"][0]["completed"], false);
    child
        .send(
            "run.resume",
            None,
            json!({ "turnId": "t_p", "runId": run_id, "afterSeq": 3 }),
        )
        .await;
    // Exactly what it missed, its directive included, then live to the end.
    let missed = child.expect("run.event").await;
    assert_eq!(missed["body"]["sequence"], 4);
    assert_eq!(missed["body"]["event"]["token"], "不错");
    assert_eq!(child.expect("run.event").await["body"]["sequence"], 5);
    assert_eq!(child.expect("perform").await["body"]["sequence"], 5);
    run.publish(AgentProgressEvent::TaskCompleted {
        task_id: "task".into(),
        success: true,
        response: Box::new(json!({ "success": true, "message": "还不错" })),
    })
    .await;
    let last = child.expect("run.event").await;
    assert_eq!(last["body"]["sequence"], 6);
    assert_eq!(last["body"]["event"]["type"], "task_completed");
    assert_eq!(child.expect("run.end").await["body"]["lastSequence"], 6);

    // A finished run caught up on is its record only: she does not act it again.
    child
        .send(
            "run.resume",
            None,
            json!({ "turnId": "t_p", "afterSeq": 0 }),
        )
        .await;
    for sequence in 1..=6 {
        let frame = child.expect("run.event").await;
        assert_eq!(frame["body"]["sequence"], sequence);
    }
    assert_eq!(child.expect("run.end").await["body"]["lastSequence"], 6);

    // Only this device's own turns; after a restart, from the registry.
    for body in [
        json!({ "turnId": "t_nope", "afterSeq": 0 }),
        json!({ "turnId": "t_p", "runId": "run_someone_elses", "afterSeq": 0 }),
    ] {
        child.send("run.resume", None, body.clone()).await;
        let rejected = child.expect("run.rejected").await;
        assert_eq!(
            rejected["body"],
            json!({ "turnId": body["turnId"], "code": "turn_unknown" })
        );
    }
    turns::forget_in_memory(&mac.id);
    child
        .send(
            "run.resume",
            None,
            json!({ "turnId": "t_p", "afterSeq": 5 }),
        )
        .await;
    assert_eq!(child.expect("run.event").await["body"]["sequence"], 6);
    assert_eq!(child.expect("run.end").await["body"]["lastSequence"], 6);
    let (other, _) = device(&site.db, "Other Mac").await;
    assert!(turns::find(&site.db, &other.id, "t_p").await.is_none());
    drop(child);
    site.close().await;
}

/// Her lines go to the app they are using her on, as `say`, kept until the
/// child acks them; on the web, they go where they always did.
#[tokio::test]
async fn her_lines_go_to_the_app_they_are_using_her_on() {
    use crate::services::agent::consciousness::{
        forget_native_presence, live_presence_from_custom_data, remember_live_presence,
        remember_native_presence,
    };
    use crate::services::agent::merope::ingest::{InPerson, in_person, say_on_app};
    use crate::services::agent::merope::reach::deliver_first_words;
    use myriad_merope::reach::Route;
    let Some(site) = site().await else {
        return;
    };
    let (mac, token) = device(&site.db, "Mac").await;
    let mut child = Child::open(&site, &token).await;
    let welcome = child.hello_with(&["rig", "say"]).await;
    assert_eq!(welcome["body"]["capabilities"], json!(["rig", "say"]));

    // The owner is only on the Mac.
    let on_the_mac =
        json!({ "presence": { "pageVisible": true, "panelVisible": false, "faceVisible": true } });
    remember_native_presence(OWNER, &mac.id, &on_the_mac);
    assert_eq!(in_person(OWNER, false), InPerson::App(mac.id.clone()));
    assert_eq!(
        in_person(OWNER, true),
        InPerson::Nowhere,
        "a touch answers the face it came from"
    );

    // Her first words: on the app.
    assert_eq!(
        deliver_first_words(&site.db, OWNER, Route::Site, "在忙吗？").await,
        Some("app")
    );
    let first = child.expect("say").await;
    let first_id = first["id"].as_str().unwrap().to_string();
    assert!(first_id.starts_with("say_"));
    assert_eq!(first["body"]["kind"], "proactive");
    assert_eq!(first["body"]["text"], "在忙吗？");
    assert!(first["body"]["saidAt"].as_i64().unwrap() > 0);
    // Acted at once with her local base motion, as any line of hers said there.
    let acted: PerformanceDirective = serde_json::from_value(first["body"]["perform"].clone())
        .expect("her first words come with how she acts them");
    assert_eq!(
        serde_json::to_value(acted.phase).unwrap(),
        json!("proactive")
    );
    // Her greeting: on the app, as she acts it; nothing on the site's face.
    let directive = fixture("say")["body"]["perform"].clone();
    let performance: PerformanceDirective = serde_json::from_value(directive.clone()).unwrap();
    assert!(
        say_on_app(
            &site.db,
            OWNER,
            &mac.id,
            "spk_greeting1",
            "agent.merope.greeting",
            "你回来啦。",
            Some(&performance),
        )
        .await
    );
    let greeting = child.expect("say").await;
    assert_eq!(greeting["id"], "spk_greeting1");
    assert_eq!(greeting["body"]["kind"], "greeting");
    assert_eq!(greeting["body"]["eventKey"], "agent.merope.greeting");
    assert_eq!(greeting["body"]["perform"], directive);
    let required: BTreeSet<String> = CONTRACT.frames["say"].required.iter().cloned().collect();
    assert!(keys(&greeting["body"]).is_superset(&required));
    assert_eq!(keys(&greeting), keys(&fixture("say")));
    // Something she says in person is face-to-face.
    assert!(
        say_on_app(
            &site.db,
            OWNER,
            &mac.id,
            "spk_thought1",
            "agent.merope.thought",
            "我刚想起一件事。",
            None
        )
        .await
    );
    let thought = child.expect("say").await;
    assert_eq!(thought["body"]["kind"], "face-to-face");
    assert!(thought["body"].get("perform").is_none());

    // Not acked when the link drops: sent again, same ids, on the next one.
    drop(child);
    let mut child = Child::open(&site, &token).await;
    child.hello_with(&["rig", "say"]).await;
    let mut again = Vec::new();
    for _ in 0..3 {
        again.push(
            child.expect("say").await["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    assert_eq!(again, [first_id.as_str(), "spk_greeting1", "spk_thought1"]);
    // Acked, they are gone.
    child.send("ack", None, json!({})).await;
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;
    drop(child);
    let mut child = Child::open(&site, &token).await;
    child.hello_with(&["rig", "say"]).await;
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;
    // One not acked in time is dropped: by then they are not here either.
    assert!(
        say_on_app(
            &site.db,
            OWNER,
            &mac.id,
            "spk_late",
            "agent.merope.thought",
            "还在吗？",
            None
        )
        .await
    );
    assert_eq!(child.expect("say").await["id"], "spk_late");
    drop(child);
    super::say::age_all(&mac.id);
    let mut child = Child::open(&site, &token).await;
    child.hello_with(&["rig", "say"]).await;
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;

    // A child that does not take `say`: her lines go where they went before.
    // (A dropped link clears the app's presence once the host notices.)
    drop(child);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut old = Child::open(&site, &token).await;
    old.hello_with(&["rig"]).await;
    remember_native_presence(OWNER, &mac.id, &on_the_mac);
    assert_eq!(in_person(OWNER, false), InPerson::Nowhere);
    assert!(
        !say_on_app(
            &site.db,
            OWNER,
            &mac.id,
            "spk_old",
            "agent.merope.thought",
            "嗯？",
            None
        )
        .await
    );
    drop(old);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // On the web: the face, as always, and first words are no `say`.
    let mut child = Child::open(&site, &token).await;
    child.hello_with(&["rig", "say"]).await;
    remember_native_presence(OWNER, &mac.id, &on_the_mac);
    assert_eq!(in_person(OWNER, false), InPerson::App(mac.id.clone()));
    let page = |visible: bool| {
        live_presence_from_custom_data(&json!({
            "presence": { "instanceId": "link-test-page", "pageVisible": visible, "faceVisible": visible }
        }))
    };
    tokio::time::sleep(Duration::from_millis(5)).await;
    remember_live_presence(OWNER, page(true));
    assert_eq!(
        in_person(OWNER, false),
        InPerson::Face,
        "the page came last"
    );
    assert_eq!(
        deliver_first_words(&site.db, OWNER, Route::Site, "嗨").await,
        None
    );
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;
    // Back on the Mac (its window comes to the front): the app again.
    tokio::time::sleep(Duration::from_millis(5)).await;
    remember_native_presence(
        OWNER,
        &mac.id,
        &json!({ "presence": { "pageVisible": false } }),
    );
    remember_native_presence(OWNER, &mac.id, &on_the_mac);
    assert_eq!(in_person(OWNER, false), InPerson::App(mac.id.clone()));

    remember_live_presence(OWNER, page(false));
    forget_native_presence(OWNER, &mac.id);
    drop(child);

    // D16: the app with its link up is where they are, its window on screen or
    // not. Coming up behind other windows, it still gets her lines as `say`.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut child = Child::open(&site, &token).await;
    child.hello_with(&["rig", "say"]).await;
    remember_native_presence(
        OWNER,
        &mac.id,
        &json!({ "presence": { "pageVisible": false, "panelVisible": false, "faceVisible": false } }),
    );
    assert!(crate::services::agent::consciousness::owner_here(OWNER).present);
    assert_eq!(in_person(OWNER, false), InPerson::App(mac.id.clone()));
    assert_eq!(
        deliver_first_words(&site.db, OWNER, Route::Site, "在吗？").await,
        Some("app")
    );
    let behind = child.expect("say").await;
    assert_eq!(behind["body"]["kind"], "proactive");
    assert_eq!(behind["body"]["text"], "在吗？");
    forget_native_presence(OWNER, &mac.id);
    drop(child);
    site.close().await;
}

/// Her body on the app: her director's refinements of the reply while the
/// app says it, also after the run's end; where saying it got to; a reply
/// cut off marked in her history; her lines' refinements; touch answered.
#[tokio::test]
async fn the_app_is_directed_while_it_says_her_reply() {
    use crate::services::agent::AgentInteractionMode;
    let Some(site) = site().await else {
        return;
    };
    let (mac, token) = device(&site.db, "Mac").await;
    let session = crate::services::agent::sessions::ensure_session(
        &site.db,
        None,
        OWNER,
        AgentInteractionMode::Chat,
    )
    .await
    .unwrap();
    let run = crate::services::agent::run_hub::create_live_run_for_test(OWNER).await;
    let run_id = run.run_id().to_string();
    turns::remember(
        &site.db,
        &mac.id,
        OWNER,
        Turn {
            turn_id: "t_body".into(),
            run_id: run_id.clone(),
            session_id: Some(session.clone()),
        },
    )
    .await;
    let directive: PerformanceDirective =
        serde_json::from_value(fixture("perform")["body"]["directive"].clone()).unwrap();

    let mut child = Child::open(&site, &token).await;
    let welcome = child.hello_with(&["rig", "say", "playback", "touch"]).await;
    assert_eq!(
        welcome["body"]["capabilities"],
        json!(["rig", "say", "playback", "touch"])
    );
    child
        .send(
            "run.resume",
            None,
            json!({ "turnId": "t_body", "afterSeq": 0 }),
        )
        .await;
    assert_eq!(child.expect("run.event").await["body"]["sequence"], 1);

    // The director's refinement goes down as it is published, numbered by
    // the direction's own version.
    assert!(run.playback_direction.publish(directive.clone()));
    let refined = child.expect("perform").await;
    assert_eq!(refined["body"]["live"], 1);
    assert_eq!(refined["body"]["turnId"], "t_body");
    assert_eq!(refined["body"]["runId"], run_id.as_str());
    assert_eq!(
        refined["body"]["directive"],
        serde_json::to_value(&directive).unwrap()
    );
    assert!(refined["body"].get("sequence").is_none());

    // Where saying it got to reaches the director's slot.
    let observe = json!({ "runId": run_id, "upcomingText": "下午去散了步。",
        "rig": { "expression": "warm", "pageVisible": true, "faceVisible": true } });
    child.send("playback.observe", None, observe).await;
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;
    assert_eq!(
        run.playback_direction
            .observation()
            .map(|observation| observation.upcoming_text),
        Some("下午去散了步。".to_string())
    );
    let big = child
        .send(
            "playback.observe",
            None,
            json!({ "runId": run_id, "upcomingText": "字".repeat(3000), "rig": {} }),
        )
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "too_large", "ref": big })
    );
    let theirs = child
        .send(
            "playback.observe",
            None,
            json!({ "runId": "run_someone_elses", "upcomingText": "x", "rig": {} }),
        )
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "not_this_device", "ref": theirs })
    );

    // The run ends; the reply is still being said, and directed.
    run.publish(AgentProgressEvent::TaskCompleted {
        task_id: "task".into(),
        success: true,
        response: Box::new(json!({ "success": true, "message": "还不错，下午去散了步。" })),
    })
    .await;
    assert_eq!(
        child.expect("run.event").await["body"]["event"]["type"],
        "task_completed"
    );
    child.expect("run.end").await;
    assert!(run.playback_direction.publish(directive.clone()));
    assert_eq!(child.expect("perform").await["body"]["live"], 2);
    // Said to the end: the direction closes, nothing more comes for it.
    child
        .send("playback.end", None, json!({ "runId": run_id }))
        .await;
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;
    assert!(run.playback_direction.read_after(2).await.closed);
    assert!(!run.playback_direction.publish(directive.clone()));

    // Cut off while saying it: marked in her history, with how much was heard.
    crate::services::agent::sessions::persist_assistant_message(
        &site.db,
        &session,
        None,
        "还不错，下午去散了步。",
        Some(json!({ "runId": run_id })),
    )
    .await
    .unwrap();
    let delivered = json!({ "turnId": "t_body", "runId": run_id, "spokenChars": 4,
        "totalChars": 11, "interrupted": true, "by": "stop" });
    child
        .send("speech.delivered", Some("sd_body1"), delivered.clone())
        .await;
    // Sent again before the ack: taken once.
    child
        .send("speech.delivered", Some("sd_body1"), delivered)
        .await;
    child.send("ping", None, json!({})).await;
    child.expect("pong").await;
    let metadata = site
        .db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "SELECT metadata::text AS m FROM agent_messages WHERE session_id = '{session}' \
                 AND role = 'assistant'"
            ),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "m")
        .unwrap();
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(
        metadata[crate::services::agent::chat_prompt::CUT_OFF_KEY],
        crate::services::agent::chat_prompt::cut_off_kind(true)
    );
    assert_eq!(metadata["heardChars"], 4);

    // A line of hers cut off on the app is marked the same way, once it is
    // kept (here a moment after the child said so).
    child
        .send(
            "speech.delivered",
            Some("sd_say_body1"),
            json!({ "sayId": "say_body1", "spokenChars": 3, "totalChars": 6,
                    "interrupted": true, "by": "barge-in" }),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    crate::services::agent::merope::store::insert_proactive(
        &site.db,
        OWNER,
        "嗨，在忙吗？",
        Some("merope.reach"),
        true,
        Some("say_body1"),
    )
    .await
    .unwrap();
    let line_metadata = |db: DatabaseConnection| async move {
        db.query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "SELECT metadata::text AS m FROM agent_proactive_messages \
                 WHERE user_id = {OWNER} AND say_id = 'say_body1'"
            ),
        ))
        .await
        .unwrap()
        .and_then(|row| row.try_get::<Option<String>>("", "m").ok().flatten())
        .map(|m| serde_json::from_str::<Value>(&m).unwrap())
    };
    let by = tokio::time::Instant::now() + WAIT;
    let marked = loop {
        if let Some(marked) = line_metadata(site.db.clone()).await {
            break marked;
        }
        assert!(tokio::time::Instant::now() < by, "the line is marked");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(
        marked,
        json!({ crate::services::agent::chat_prompt::CUT_OFF_KEY:
                crate::services::agent::chat_prompt::cut_off_kind(true), "heardChars": 3 })
    );
    // Said to the end, a line is left as it was.
    child
        .send(
            "speech.delivered",
            Some("sd_say_body2"),
            json!({ "sayId": "say_body1", "spokenChars": 6, "totalChars": 6, "interrupted": false }),
        )
        .await;
    let unnamed = child
        .send(
            "speech.delivered",
            Some("sd_say_body3"),
            json!({ "sayId": "say body", "spokenChars": 1, "totalChars": 6, "interrupted": true }),
        )
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_id", "ref": unnamed })
    );
    assert_eq!(
        line_metadata(site.db.clone()).await.unwrap()["heardChars"],
        3
    );

    // Her line on the app, refined: sent for that line to a child that takes it.
    assert!(super::perform_say(
        OWNER,
        &mac.id,
        "spk_body1",
        serde_json::to_value(&directive).unwrap()
    ));
    let line = child.expect("perform").await;
    assert_eq!(line["body"]["sayId"], "spk_body1");
    assert!(line["body"].get("turnId").is_none());

    // Touch going on: answered for that contact (her persona is off here: no reaction).
    remember_native_presence_for_test(&mac.id, true);
    child
        .send(
            "touch.appraise",
            None,
            fixture("touch.appraise")["body"].clone(),
        )
        .await;
    assert_eq!(
        child.expect("touch.reaction").await["body"],
        json!({ "touchId": "touch_01", "reaction": null })
    );
    let unnamed = child
        .send(
            "touch.appraise",
            None,
            json!({ "touchId": "no way", "touch": fixture("touch.appraise")["body"]["touch"] }),
        )
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_id", "ref": unnamed })
    );
    // An ended touch is answered on the app it came from.
    crate::services::agent::merope::touch::touched_on(OWNER, Some(&mac.id));
    assert_eq!(
        crate::services::agent::merope::ingest::in_person(OWNER, true),
        crate::services::agent::merope::ingest::InPerson::App(mac.id.clone())
    );
    crate::services::agent::merope::touch::touched_on(OWNER, None);
    drop(child);

    // A child that did not say it takes them gets none of this, and may not send it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut old = Child::open(&site, &token).await;
    old.hello_with(&["rig", "say"]).await;
    assert!(!super::perform_say(
        OWNER,
        &mac.id,
        "spk_body2",
        serde_json::to_value(&directive).unwrap()
    ));
    let refused = old
        .send("touch", Some("touch_02"), fixture("touch")["body"].clone())
        .await;
    assert_eq!(
        old.expect("error").await["body"],
        json!({ "code": "bad_frame", "ref": refused })
    );
    drop(old);
    crate::services::agent::consciousness::forget_native_presence(OWNER, &mac.id);
    site.close().await;
}

fn remember_native_presence_for_test(device_id: &str, visible: bool) {
    crate::services::agent::consciousness::remember_native_presence(
        OWNER,
        device_id,
        &json!({ "presence": { "pageVisible": visible, "faceVisible": visible } }),
    );
}

/// Work on the app: it starts from her Work memories, asked for over the
/// link, and what it did becomes her memory, kept as from that device, told
/// back as taken only once it is written, and once.
#[tokio::test]
async fn what_the_app_did_becomes_part_of_her() {
    use crate::services::agent::memory::unified::{self, Audience, MemoryKind, NewMemory, Speaker};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let Some(site) = site().await else {
        return;
    };
    let (mac, token) = device(&site.db, "Mac").await;
    let private = Audience::private(OWNER);
    unified::remember(
        &site.db,
        NewMemory {
            user_id: OWNER,
            kind: MemoryKind::Pattern,
            content: "周报要按项目分组，表格放在最前面".into(),
            evidence: None,
            speaker: Speaker::Agent,
            source: "work",
            audience: private.clone(),
            importance: 0.8,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap();

    // A child that did not say it sends experience is not asked.
    let mut plain = Child::open(&site, &token).await;
    plain.hello_with(&["rig"]).await;
    let asked = plain
        .send(
            "work.recall",
            None,
            json!({ "requestId": "wr_0", "query": "周报" }),
        )
        .await;
    assert_eq!(
        plain.expect("error").await["body"],
        json!({ "code": "bad_frame", "ref": asked })
    );
    drop(plain);

    let mut child = Child::open(&site, &token).await;
    let welcome = child.hello_with(&["rig", "experience"]).await;
    assert_eq!(
        welcome["body"]["capabilities"],
        json!(["rig", "experience"])
    );
    child
        .send(
            "work.recall",
            None,
            json!({ "requestId": "wr_1", "query": "把这周的周报整理成表格" }),
        )
        .await;
    let recalled = child.expect("work.recalled").await;
    assert_eq!(recalled["body"]["requestId"], "wr_1");
    let memories = recalled["body"]["memories"].as_array().unwrap();
    assert!(memories.len() <= 4);
    assert!(
        memories
            .iter()
            .any(|memory| memory == "周报要按项目分组，表格放在最前面"),
        "{recalled}"
    );
    let bad = child
        .send(
            "work.recall",
            None,
            json!({ "requestId": "has space", "query": "x" }),
        )
        .await;
    assert_eq!(
        child.expect("error").await["body"],
        json!({ "code": "bad_id", "ref": bad })
    );

    // A failed run they started, correcting her: a fact and a lesson, the
    // step whose outcome is unknown no lesson.
    let failed = json!({
        "kind": "work", "started": "owner", "status": "failed",
        "input": "不是，周报应该按日期排", "reply": "没能写进去。", "endedAt": 1_760_140_880_000_i64,
        "conversation": [{ "role": "tools", "content": "pi's own tools: bash ×1 (1 failed)" }],
        "outputs": [
            { "id": "step_1", "value": { "action": "不是，周报应该按日期排", "capability": "data.transform", "success": false, "error": "表格列名不对" } },
            { "id": "step_2", "value": { "action": "不是，周报应该按日期排", "capability": "resource.create", "success": false, "error": "调用被中止，结果未知", "execution_unknown": true }, "unknown": true }
        ],
        "capabilities": ["data.transform", "resource.create"]
    });
    child
        .send("experience", Some("exp_1"), failed.clone())
        .await;
    let taken = child.expect("experience.taken").await;
    assert_eq!(taken["body"], json!({ "id": "exp_1" }));
    assert!(taken.get("id").is_none(), "an answer, not kept");
    let rows = crate::models::entities::agent_memories::Entity::find()
        .filter(crate::models::entities::agent_memories::Column::Origin.eq(mac.id.clone()))
        .all(&site.db)
        .await
        .unwrap();
    let mut kinds: Vec<&str> = rows.iter().map(|row| row.kind.as_str()).collect();
    kinds.sort();
    assert_eq!(kinds, ["fact", "lesson"], "{rows:?}");
    for row in &rows {
        assert_eq!(row.user_id, Some(OWNER));
        assert_eq!(row.source, "work");
        assert_eq!(row.audience, json!([OWNER]));
        assert_eq!(row.venue, private.venue());
    }
    assert!(rows.iter().any(|row| row.content.contains("表格列名不对")));
    assert!(!rows.iter().any(|row| row.content.contains("结果未知")));

    // Sent again (its taken was lost): told again, written once.
    child.send("experience", Some("exp_1"), failed).await;
    assert_eq!(
        child.expect("experience.taken").await["body"],
        json!({ "id": "exp_1" })
    );
    let again = crate::models::entities::agent_memories::Entity::find()
        .filter(crate::models::entities::agent_memories::Column::Origin.eq(mac.id.clone()))
        .all(&site.db)
        .await
        .unwrap();
    assert_eq!(again.len(), 2);

    // Stopped part way, and scheduled: taken, nothing learned from the first.
    for (id, started, status) in [
        ("exp_2", "owner", "cancelled"),
        ("exp_3", "schedule", "completed"),
    ] {
        child
            .send(
                "experience",
                Some(id),
                json!({ "kind": "work", "started": started, "status": status,
                        "input": "整理下载文件夹", "endedAt": 1_760_140_890_000_i64, "title": "每晚整理" }),
            )
            .await;
        assert_eq!(
            child.expect("experience.taken").await["body"],
            json!({ "id": id })
        );
    }
    let after = crate::models::entities::agent_memories::Entity::find()
        .filter(crate::models::entities::agent_memories::Column::Origin.eq(mac.id.clone()))
        .all(&site.db)
        .await
        .unwrap();
    assert_eq!(after.len(), 2);

    // Not one: what it cannot read is said so, and nothing is taken.
    for (id, body, code) in [
        (
            "has space",
            json!({ "kind": "work", "started": "owner", "status": "completed", "input": "x", "endedAt": 1 }),
            "bad_id",
        ),
        (
            "exp_4",
            json!({ "kind": "chat", "started": "owner", "status": "completed", "input": "x", "endedAt": 1 }),
            "bad_frame",
        ),
        (
            "exp_5",
            json!({ "kind": "work", "started": "nobody", "status": "completed", "input": "x", "endedAt": 1 }),
            "bad_frame",
        ),
        (
            "exp_6",
            json!({ "kind": "work", "started": "owner", "status": "completed", "input": "  ", "endedAt": 1 }),
            "bad_frame",
        ),
    ] {
        let seq = child.send("experience", Some(id), body).await;
        assert_eq!(
            child.expect("error").await["body"],
            json!({ "code": code, "ref": seq })
        );
    }

    // What the Mac learned is hers on every end: the site's own Work
    // recalls it.
    let found = unified::recall(
        &site.db,
        OWNER,
        &private,
        Some("周报应该按日期排"),
        &MemoryKind::FOR_WORK,
        4,
    )
    .await
    .unwrap();
    assert!(
        found
            .iter()
            .any(|memory| memory.content.contains("周报应该按日期排"))
    );
    drop(child);
    site.close().await;
}
