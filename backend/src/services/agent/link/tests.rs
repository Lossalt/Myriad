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
        self.send(
            "hello",
            None,
            json!({ "protocol": 1, "role": "child", "surface": "app",
                    "capabilities": ["rig", "telepathy", "rig", "notify"] }),
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
    // A run's last record must be kept before it goes out (as other run
    // tests do, this process's database is this site's).
    crate::services::process_db::set_process_database(site.db.clone());
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
    // The run, in its own sequence, until it ends. This site has no model
    // to answer with, so once the run is under way its owner stops it.
    let mut sequences = Vec::new();
    let mut stopped = false;
    let mut last_event = Value::Null;
    let end = loop {
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
                last_event = frame["body"]["event"].clone();
            }
            "run.end" => break frame,
            "perform" | "mood" => {}
            other => panic!("{other}: {frame}"),
        }
        if sequences.len() == 3 && !stopped {
            stopped = true;
            crate::services::agent::run_hub::get_live_run_for_user(&run_id, OWNER)
                .await
                .expect("the owner's run")
                .abort_execution()
                .await;
        }
    };
    assert!(stopped && sequences.len() >= 4, "{sequences:?}");
    assert_eq!(
        (last_event["type"].as_str(), last_event["code"].as_str()),
        (Some("error"), Some("CANCELLED"))
    );
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "{sequences:?}"
    );
    assert_eq!(end["body"]["lastSequence"], *sequences.last().unwrap());
    assert_eq!(keys(&end["body"]), keys(&fixture("run.end")["body"]));
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
