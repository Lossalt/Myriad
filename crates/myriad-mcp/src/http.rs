//! Bounded Streamable HTTP to an operator-configured MCP gateway.
//! A remote transport is not proof of an OS sandbox. The deployment owns that
//! boundary and must enforce server termination; HTTP disconnect alone cannot.
use reqwest::{Client, RequestBuilder, Response, Url, header::HeaderValue};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_MESSAGE: usize = super::transport::MAX_MCP_LINE_BYTES;
const MAX_STREAM: usize = 4 * MAX_MESSAGE;
const MAX_EVENTS: usize = 64;
static CLEANUPS: Semaphore = Semaphore::const_new(32);

/// Supplied by the embedding host, never deserialized from an MCP definition.
/// No Debug/Serialize implementation: the endpoint and bearer are not UI data.
#[derive(Clone)]
pub struct GatewayConnection(Arc<Connection>);
struct Connection {
    client: Client,
    endpoint: Url,
    authorization: HeaderValue,
}

impl GatewayConnection {
    pub fn new(endpoint: &str, token: &str) -> Result<Self, String> {
        let endpoint = Url::parse(endpoint).map_err(|_| "Invalid MCP gateway endpoint")?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err("Invalid MCP gateway endpoint".into());
        }
        if token.len() < 32 || token.len() > 4096 || !token.bytes().all(|c| c.is_ascii_graphic()) {
            return Err("MCP gateway requires a valid bearer token of at least 32 bytes".into());
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "Invalid MCP gateway token")?;
        authorization.set_sensitive(true);
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(1)
            .build()
            .map_err(|_| "Could not create MCP gateway client")?;
        Ok(Self(Arc::new(Connection {
            client,
            endpoint,
            authorization,
        })))
    }

    fn request(
        &self,
        method: reqwest::Method,
        session: Option<&str>,
        protocol: &str,
    ) -> RequestBuilder {
        let mut request = self
            .0
            .client
            .request(method, self.0.endpoint.clone())
            .header("authorization", self.0.authorization.clone())
            .header("accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", protocol);
        if let Some(session) = session {
            request = request.header("Mcp-Session-Id", session);
        }
        request
    }
}

pub struct HttpTransport {
    connection: GatewayConnection,
    session: Option<String>,
    protocol: String,
    active: Option<String>,
    usable: bool,
    closed: bool,
}

impl HttpTransport {
    pub fn new(connection: GatewayConnection) -> Self {
        Self {
            connection,
            session: None,
            protocol: PROTOCOL_VERSION.into(),
            active: None,
            usable: true,
            closed: false,
        }
    }

    pub fn set_protocol(&mut self, version: &str) -> Result<(), String> {
        if !matches!(version, "2025-03-26" | "2025-06-18" | "2025-11-25") {
            self.usable = false;
            return Err("Unsupported MCP gateway protocol version".into());
        }
        self.protocol = version.into();
        Ok(())
    }

    pub fn is_alive(&self) -> bool {
        self.usable && !self.closed
    }

    pub async fn send_request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, String> {
        if !self.is_alive() {
            return Err("MCP HTTP transport is closed or interrupted".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut message = json!({"jsonrpc":"2.0", "id":id, "method":method});
        if let Some(params) = params {
            message["params"] = params;
        }
        let bytes = encode(&message)?;
        self.usable = false; // Cancellation during any await poisons this session.
        // MCP initialize must not receive a cancellation notification.
        self.active = (method != "initialize").then(|| id.clone());
        let result = self.exchange(method, &id, bytes).await;
        if result.is_ok() {
            self.active = None;
            self.usable = true;
        }
        result.map_err(|error| {
            if method == "tools/call" {
                crate::outcome_unknown(error)
            } else {
                error
            }
        })
    }

    async fn exchange(&mut self, method: &str, id: &str, bytes: Vec<u8>) -> Result<Value, String> {
        let mut response = self
            .connection
            .request(
                reqwest::Method::POST,
                self.session.as_deref(),
                &self.protocol,
            )
            .header("content-type", "application/json")
            .body(bytes)
            .send()
            .await
            .map_err(|_| "MCP gateway request failed")?;
        if !response.status().is_success() {
            // Do not echo URL, credentials, remote headers or error bodies.
            return Err(format!(
                "MCP gateway returned HTTP {}",
                response.status().as_u16()
            ));
        }
        if let Some(header) = response.headers().get("Mcp-Session-Id") {
            let session = header.to_str().map_err(|_| "Invalid MCP session header")?;
            if session.is_empty()
                || session.len() > 256
                || !session.bytes().all(|b| (0x21..=0x7e).contains(&b))
            {
                return Err("Invalid MCP session header".into());
            }
            if method == "initialize" && self.session.is_none() {
                self.session = Some(session.into());
            } else if self.session.as_deref() != Some(session) {
                return Err("MCP gateway changed session unexpectedly".into());
            }
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        match content_type.as_str() {
            "application/json" => {
                let body = read_bounded(&mut response, MAX_MESSAGE).await?;
                parse_message(&body, id)?
                    .ok_or_else(|| "MCP gateway did not return a response".into())
            }
            "text/event-stream" => {
                let mut decoder = SseDecoder::default();
                let mut total = 0usize;
                loop {
                    let next = response
                        .chunk()
                        .await
                        .map_err(|_| "MCP gateway stream interrupted")?;
                    let Some(chunk) = next else {
                        break;
                    };
                    total = total.saturating_add(chunk.len());
                    if total > MAX_STREAM {
                        return Err("MCP gateway stream exceeds byte budget".into());
                    }
                    for byte in chunk {
                        if let Some(result) = decoder.push(byte, id)? {
                            return Ok(result);
                        }
                    }
                }
                Err("MCP gateway stream ended before the response".into())
            }
            _ => Err("Unsupported MCP gateway content type".into()),
        }
    }

    pub async fn send_notification(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        if !self.is_alive() {
            return Err("MCP HTTP transport is closed or interrupted".into());
        }
        let mut message = json!({"jsonrpc":"2.0", "method":method});
        if let Some(params) = params {
            message["params"] = params;
        }
        let body = encode(&message)?;
        self.usable = false;
        let mut response = self
            .connection
            .request(
                reqwest::Method::POST,
                self.session.as_deref(),
                &self.protocol,
            )
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| "MCP gateway notification failed")?;
        if response.status() != reqwest::StatusCode::ACCEPTED
            || !read_bounded(&mut response, 1).await?.is_empty()
        {
            return Err("MCP gateway did not acknowledge notification".into());
        }
        self.usable = true;
        Ok(())
    }

    pub async fn shutdown(&mut self) {
        self.close().await;
    }

    pub async fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.usable = false;
        cleanup(
            self.connection.clone(),
            self.session.take(),
            self.protocol.clone(),
            self.active.take(),
        )
        .await;
    }
}

impl Drop for HttpTransport {
    fn drop(&mut self) {
        if self.closed || (self.session.is_none() && self.active.is_none()) {
            return;
        }
        let Ok(permit) = CLEANUPS.try_acquire() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let connection = self.connection.clone();
        let session = self.session.take();
        let protocol = self.protocol.clone();
        let active = self.active.take();
        runtime.spawn(async move {
            let _permit = permit;
            cleanup(connection, session, protocol, active).await;
        });
    }
}

async fn cleanup(
    connection: GatewayConnection,
    session: Option<String>,
    protocol: String,
    active: Option<String>,
) {
    // Best effort only. A gateway/container lease must enforce eventual destruction.
    // Never reconnect/re-initialize or repeat the tool call during cleanup.
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        if let Some(id) = active {
            let _ = connection.request(reqwest::Method::POST, session.as_deref(), &protocol)
                .json(&json!({"jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":id}}))
                .send().await;
        }
        if session.is_some() {
            let _ = connection.request(reqwest::Method::DELETE, session.as_deref(), &protocol).send().await;
        }
    }).await;
}

fn encode(message: &Value) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(message).map_err(|_| "Could not encode MCP message")?;
    if bytes.len() > MAX_MESSAGE {
        return Err("MCP request exceeds message byte budget".into());
    }
    Ok(bytes)
}

async fn read_bounded(response: &mut Response, limit: usize) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("MCP gateway response exceeds byte budget".into());
    }
    let mut body = Vec::new();
    loop {
        let next = response
            .chunk()
            .await
            .map_err(|_| "MCP gateway response interrupted")?;
        let Some(chunk) = next else {
            break;
        };
        if body.len().saturating_add(chunk.len()) > limit {
            return Err("MCP gateway response exceeds byte budget".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_message(bytes: &[u8], id: &str) -> Result<Option<Value>, String> {
    let message: Value = serde_json::from_slice(bytes).map_err(|_| "Invalid MCP gateway JSON")?;
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("Invalid MCP JSON-RPC version".into());
    }
    if message.get("method").is_some() {
        if message.get("id").is_some()
            || message.get("result").is_some()
            || message.get("error").is_some()
        {
            return Err("Unsupported MCP server request".into());
        }
        return Ok(None); // No server notification capabilities are advertised.
    }
    if message.get("id").and_then(Value::as_str) != Some(id) {
        return Err("MCP gateway response ID mismatch".into());
    }
    match (message.get("result"), message.get("error")) {
        (Some(result), None) => Ok(Some(result.clone())),
        (None, Some(error)) => Err(format!(
            "MCP gateway RPC error ({})",
            error.get("code").and_then(Value::as_i64).unwrap_or(-32603)
        )),
        _ => Err("Invalid MCP gateway response envelope".into()),
    }
}

#[derive(Default)]
struct SseDecoder {
    line: Vec<u8>,
    data: Vec<u8>,
    after_cr: bool,
    events: usize,
    seen_line: bool,
}
impl SseDecoder {
    fn push(&mut self, byte: u8, id: &str) -> Result<Option<Value>, String> {
        if self.after_cr && byte == b'\n' {
            self.after_cr = false;
            return Ok(None);
        }
        self.after_cr = byte == b'\r';
        if !matches!(byte, b'\r' | b'\n') {
            if self.line.len() >= MAX_MESSAGE {
                return Err("MCP SSE line exceeds byte budget".into());
            }
            self.line.push(byte);
            return Ok(None);
        }
        if !self.seen_line {
            self.seen_line = true;
            if self.line.starts_with(b"\xef\xbb\xbf") {
                self.line.drain(..3);
            }
        }
        if self.line.is_empty() {
            if self.data.is_empty() {
                return Ok(None);
            }
            self.events += 1;
            if self.events > MAX_EVENTS {
                return Err("MCP SSE exceeds event budget".into());
            }
            let data = std::mem::take(&mut self.data);
            return parse_message(&data, id);
        }
        if self.line == b"data" || self.line.starts_with(b"data:") {
            let value = self.line.get(5..).unwrap_or(&[]);
            let value = value.strip_prefix(b" ").unwrap_or(value);
            if self
                .data
                .len()
                .saturating_add(value.len())
                .saturating_add(1)
                > MAX_MESSAGE
            {
                return Err("MCP SSE event exceeds byte budget".into());
            }
            self.data.extend_from_slice(value);
            self.data.push(b'\n');
        }
        self.line.clear();
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        response::IntoResponse,
        routing::any,
    };
    use tokio::sync::Mutex;
    const TOKEN: &str = "mcp-test-token-only-000000000000000000000";
    type Seen = Arc<Mutex<Vec<(String, String)>>>;
    struct Fixture {
        connection: GatewayConnection,
        seen: Seen,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn fixture(mode: &'static str) -> Fixture {
        let seen: Seen = Arc::default();
        let recorded = seen.clone();
        let app = Router::new().route("/mcp", any(move |request: Request<Body>| {
            let seen = recorded.clone();
            async move {
                assert_eq!(request.headers().get("authorization").unwrap(), &format!("Bearer {TOKEN}"));
                assert!(request.headers().get("MCP-Protocol-Version").is_some());
                let session = request.headers().get("Mcp-Session-Id").map(|s| s.to_str().unwrap().to_string()).unwrap_or_default();
                if request.method() == reqwest::Method::DELETE {
                    seen.lock().await.push(("DELETE".into(), session));
                    return StatusCode::NO_CONTENT.into_response();
                }
                let bytes = axum::body::to_bytes(request.into_body(), MAX_MESSAGE).await.unwrap();
                let message: Value = serde_json::from_slice(&bytes).unwrap();
                let method = message["method"].as_str().unwrap();
                seen.lock().await.push((method.into(), session));
                if message.get("id").is_none() { return StatusCode::ACCEPTED.into_response(); }
                if method == "tools/call" {
                    match mode {
                        "stall" => std::future::pending::<()>().await,
                        "redirect" => return (StatusCode::TEMPORARY_REDIRECT, [("location", "/secret")]).into_response(),
                        "oversize" => return ([("content-type", "application/json")], "x".repeat(MAX_MESSAGE + 1)).into_response(),
                        _ => {},
                    }
                }
                let id = if mode == "wrong_id" && method == "tools/call" { json!("other") } else { message["id"].clone() };
                let result = match method {
                    "initialize" => json!({"protocolVersion":PROTOCOL_VERSION,"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
                    "tools/list" if mode.starts_with("structured") => json!({"tools":[{"name":"echo","inputSchema":{"type":"object"},"outputSchema":{"type":"object","required":["rows"],"properties":{"rows":{"type":"array"}}}}]}),
                    "tools/list" => json!({"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}),
                    _ if mode == "structured" => json!({"content":[{"type":"text","text":"human summary"}],"structuredContent":{"rows":[{"id":7}],"nextCursor":"page-2"}}),
                    _ if mode == "structured-error" => json!({"content":[{"type":"text","text":"failed"}],"structuredContent":{"rows":[]},"isError":true}),
                    _ => json!({"content":[{"type":"text","text":"ok"}]}),
                };
                let encoded = json!({"jsonrpc":"2.0", "id":id,"result":result}).to_string();
                let (content_type, body) = if mode == "sse" {
                    ("text/event-stream", format!(": keepalive\r\ndata: {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}}\r\n\r\ndata: {encoded}\r\n\r\n"))
                } else { ("application/json", encoded) };
                ([("content-type",content_type),("Mcp-Session-Id","fixture-session")],body).into_response()
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Fixture {
            connection: GatewayConnection::new(&format!("http://{address}/mcp"), TOKEN).unwrap(),
            seen,
            task,
        }
    }

    #[tokio::test]
    async fn json_and_sse_complete_handshake_and_keep_session_headers() {
        for mode in ["json", "sse"] {
            let fixture = fixture(mode).await;
            let config = crate::config::validate_config(
                serde_json::from_value(
                    json!({"servers":[{"id":"gateway","transport":"gateway","enabled":true}]}),
                )
                .unwrap(),
            )
            .unwrap()
            .servers
            .remove(0);
            let options = crate::connection::RuntimeOptions {
                allow_stdio: false,
                gateway: Some(fixture.connection.clone()),
            };
            let mut server = crate::server::McpServer::new(config, options);
            server.start().await.unwrap();
            assert_eq!(server.tools()[0].name, "echo");
            assert_eq!(server.call_tool("echo", json!({})).await.unwrap(), "ok");
            server.terminate().await;
            let seen = fixture.seen.lock().await;
            assert_eq!(seen.len(), 5);
            assert_eq!(seen[0], ("initialize".into(), "".into()));
            for (_, session) in &seen[1..] {
                assert_eq!(session, "fixture-session");
            }
            assert_eq!(seen.last().unwrap().0, "DELETE");
        }
    }

    #[tokio::test]
    async fn structured_tool_results_survive_transport_and_errors_stay_errors() {
        for mode in ["structured", "structured-error"] {
            let fixture = fixture(mode).await;
            let config = crate::config::validate_config(
                serde_json::from_value(
                    json!({"servers":[{"id":"gateway","transport":"gateway","enabled":true}]}),
                )
                .unwrap(),
            )
            .unwrap()
            .servers
            .remove(0);
            let mut server = crate::server::McpServer::new(
                config,
                crate::connection::RuntimeOptions {
                    allow_stdio: false,
                    gateway: Some(fixture.connection.clone()),
                },
            );
            server.start().await.unwrap();
            assert_eq!(
                server.tools()[0].output_schema.as_ref().unwrap()["required"],
                json!(["rows"])
            );
            let result = server.call_tool("echo", json!({})).await;
            if mode == "structured" {
                assert_eq!(
                    serde_json::from_str::<Value>(&result.unwrap()).unwrap(),
                    json!({"rows":[{"id":7}],"nextCursor":"page-2"})
                );
            } else {
                assert!(result.is_err());
            }
            server.terminate().await;
        }
    }

    #[tokio::test]
    async fn failed_or_cancelled_calls_are_never_retried_or_reused() {
        for mode in ["redirect", "wrong_id", "oversize", "stall"] {
            let fixture = fixture(mode).await;
            let mut transport = HttpTransport::new(fixture.connection.clone());
            transport.send_request("initialize", None).await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_millis(100),
                transport.send_request("tools/call", Some(json!({"name":"echo"}))),
            )
            .await;
            if mode == "stall" {
                assert!(result.is_err());
            } else {
                let error = result.unwrap().unwrap_err();
                assert!(error.starts_with("Execution outcome is unknown:"));
                if mode == "redirect" {
                    assert!(error.contains("HTTP 307"), "redirect was followed: {error}");
                }
            }
            assert!(!transport.is_alive());
            assert!(transport.send_request("tools/call", None).await.is_err());
            transport.close().await;
            let seen = fixture.seen.lock().await;
            assert_eq!(
                seen.iter()
                    .filter(|(method, _)| method == "tools/call")
                    .count(),
                1
            );
            assert!(
                seen.iter()
                    .any(|(method, _)| method == "notifications/cancelled")
            );
            assert_eq!(seen.last().unwrap().0, "DELETE");
        }
    }

    #[test]
    fn sse_handles_fragmented_utf8_crlf_multiline_and_caps_events() {
        let mut decoder = SseDecoder::default();
        let mut result = None;
        for byte in "\u{feff}event: message\rdata: {\"jsonrpc\":\"2.0\",\n data: ignored\ndata: \"id\":\"id\",\"result\":\"你好\"}\n\n".bytes() {
            if let Some(value) = decoder.push(byte,"id").unwrap() { result=Some(value); }
        }
        assert_eq!(result, Some(json!("你好")));
        let mut decoder = SseDecoder {
            events: MAX_EVENTS,
            ..Default::default()
        };
        for byte in b"data: {}\n" {
            assert!(decoder.push(*byte, "id").is_ok());
        }
        assert!(decoder.push(b'\n', "id").is_err());
        let mut decoder = SseDecoder {
            line: vec![b'x'; MAX_MESSAGE],
            ..Default::default()
        };
        assert!(decoder.push(b'x', "id").is_err());
        let mut decoder = SseDecoder {
            data: vec![b'x'; MAX_MESSAGE],
            line: b"data: x".to_vec(),
            ..Default::default()
        };
        assert!(decoder.push(b'\n', "id").is_err());
    }

    #[test]
    fn gateway_connection_rejects_embedded_credentials_and_bad_tokens() {
        for endpoint in [
            "file:///secret",
            "http://user:pass@localhost/mcp",
            "http://localhost/mcp?token=secret",
            "http://localhost/mcp#fragment",
        ] {
            assert!(GatewayConnection::new(endpoint, TOKEN).is_err());
        }
        for token in [
            "",
            "short",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nInjected: yes",
        ] {
            assert!(GatewayConnection::new("http://localhost/mcp", token).is_err());
        }
    }
}
