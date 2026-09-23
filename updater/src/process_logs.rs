use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use bollard::container::LogOutput;
use bollard::query_parameters::{ListContainersOptionsBuilder, LogsOptionsBuilder};
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::docker::client::DockerClient;
use crate::docker::guard::{
    MAX_CONCURRENT_LOG_READS, MAX_CONTAINER_LOG_BODY, MAX_CONTAINER_LOG_TAIL,
};
use crate::redact::redact_secrets;

const CORE_SERVICES: &[&str] = &[
    "backend",
    "backend-volume-init",
    "federation-worker",
    "persona-worker",
    "frontend",
    "postgres",
    "proxy",
    "docker-guard",
    "updater",
    "updater-gateway",
];
const MAX_SCANNED_LINES_PER_SOURCE: usize = MAX_CONTAINER_LOG_TAIL as usize;
const MAX_ENTRIES_PER_SOURCE: usize = 200;
const MAX_INPUT_LINE_BYTES: usize = 16 * 1_024;
const MAX_ENTRY_BYTES: usize = 2_048;
const MAX_SOURCE_RESPONSE_BYTES: usize = MAX_CONTAINER_LOG_BODY;
const MAX_CONCURRENT_SOURCES: usize = MAX_CONCURRENT_LOG_READS;
const SOURCE_TIMEOUT: Duration = Duration::from_secs(5);

static ANSI_ESCAPE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("valid ANSI regex"));
static PREFIX_LEVEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(?:(?:\d{4}-\d{2}-\d{2}[T ]\S+)(?:\s+[A-Z]{2,5})?\s+(?:\[[^\]]+\]\s+)?)?(?P<level>trace|debug|info|notice|log|warn|warning|error|fatal|panic)(?:$|[\s\[\]:])")
        .expect("valid prefix log level regex")
});
static STRUCTURED_LEVEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:^|[\s,{])["']?level["']?\s*[:=]\s*["']?(?P<level>trace|debug|info|notice|log|warn|warning|error|fatal|panic)(?:["'\s,}]|$)"#)
        .expect("valid structured log level regex")
});
static JSON_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)(?P<prefix>["']?(?:access[_-]?token|refresh[_-]?token|id[_-]?token|api[_-]?key|client[_-]?secret|password|passwd|secret)["']?\s*[:=]\s*["']?)[^"'\s,}&]+"#,
    )
    .expect("valid JSON secret regex")
});
static CREDENTIAL_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?P<prefix>\b(?:postgres(?:ql)?|mysql|redis)://[^:\s/@]+:)[^@\s/]+@")
        .expect("valid credential URL regex")
});
static JWT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b")
        .expect("valid JWT regex")
});
static URL_QUERY_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?P<prefix>[?&](?:key|api[_-]?key|token|access[_-]?token|refresh[_-]?token|secret|password)=)[^&#\s]+")
        .expect("valid URL query secret regex")
});

#[derive(Debug, Serialize)]
pub struct ProcessLogExport {
    format: &'static str,
    schema_version: u32,
    generated_at: DateTime<Utc>,
    scope: &'static str,
    limits: ExportLimits,
    sources: Vec<ProcessLogSource>,
    collection_errors: Vec<CollectionError>,
    omitted_sources: Vec<OmittedSource>,
}

#[derive(Debug, Serialize)]
struct ExportLimits {
    scanned_lines_per_source: usize,
    retained_entries_per_source: usize,
    input_line_bytes: usize,
    entry_bytes: usize,
    source_response_bytes: usize,
}

#[derive(Debug, Serialize)]
struct ProcessLogSource {
    service: String,
    container: String,
    state: Option<String>,
    entries: Vec<ProcessLogEntry>,
    scanned_lines: usize,
    matched_lines: usize,
    input_truncated: bool,
    truncated: bool,
    complete: bool,
}

#[derive(Debug, Serialize)]
struct ProcessLogEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
    stream: LogStream,
    level: ExportLevel,
    message: String,
    truncated: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum LogStream {
    Stdout,
    Stderr,
    Console,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ExportLevel {
    Warning,
    Error,
    UnclassifiedStderr,
}

#[derive(Debug, Serialize)]
struct CollectionError {
    service: String,
    container: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct OmittedSource {
    service: &'static str,
    reason: &'static str,
}

#[derive(Debug)]
struct ContainerSource {
    id: String,
    service: String,
    container: String,
    state: Option<String>,
    running: bool,
    created: i64,
}

pub async fn export(docker: Arc<DockerClient>) -> ProcessLogExport {
    let mut report = empty_export();
    let project = std::env::var("COMPOSE_PROJECT_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "myriad".to_string());
    let options = ListContainersOptionsBuilder::default().all(true).build();
    let containers = match docker.raw().list_containers(Some(options)).await {
        Ok(containers) => containers,
        Err(error) => {
            report.collection_errors.push(CollectionError {
                service: "docker".into(),
                container: String::new(),
                error: sanitize_message(&format!("list containers: {error}")),
            });
            report.omitted_sources = CORE_SERVICES
                .iter()
                .map(|service| OmittedSource {
                    service,
                    reason: "inventory_unavailable",
                })
                .collect();
            return report;
        }
    };

    let mut discovered = HashMap::<String, ContainerSource>::new();
    for container in containers {
        let labels = container.labels.unwrap_or_default();
        if labels.get("com.docker.compose.project").map(String::as_str) != Some(project.as_str()) {
            continue;
        }
        let Some(service) = labels
            .get("com.docker.compose.service")
            .filter(|service| CORE_SERVICES.contains(&service.as_str()))
            .cloned()
        else {
            continue;
        };
        let Some(id) = container.id else {
            report.collection_errors.push(CollectionError {
                service,
                container: String::new(),
                error: "container inventory did not include an id".into(),
            });
            continue;
        };
        let name = container
            .names
            .unwrap_or_default()
            .into_iter()
            .next()
            .map(|name| name.trim_start_matches('/').to_string())
            .unwrap_or_else(|| id.chars().take(12).collect());
        let running = container
            .state
            .as_ref()
            .is_some_and(|state| state.to_string() == "running");
        let candidate = ContainerSource {
            id,
            service: service.clone(),
            container: name,
            state: container.state.map(|state| state.to_string()),
            running,
            created: container.created.unwrap_or_default(),
        };
        match discovered.get(&service) {
            Some(existing)
                if existing.running && !candidate.running
                    || existing.running == candidate.running
                        && existing.created >= candidate.created => {}
            _ => {
                discovered.insert(service, candidate);
            }
        }
    }
    let discovered_services = discovered.keys().cloned().collect::<Vec<_>>();
    let mut discovered = discovered.into_values().collect::<Vec<_>>();
    discovered.sort_by(|left, right| {
        left.service
            .cmp(&right.service)
            .then_with(|| left.container.cmp(&right.container))
    });

    report.omitted_sources = CORE_SERVICES
        .iter()
        .filter(|service| !discovered_services.iter().any(|found| found == **service))
        .map(|service| OmittedSource {
            service,
            reason: "not_present",
        })
        .collect();

    let results = stream::iter(discovered.into_iter().map(|source| {
        let docker = docker.clone();
        async move {
            let service = source.service.clone();
            let container = source.container.clone();
            match tokio::time::timeout(SOURCE_TIMEOUT, collect_source(docker, source)).await {
                Ok(Ok(source)) => Ok(source),
                Ok(Err(error)) => Err(CollectionError {
                    service,
                    container,
                    error: sanitize_message(&error),
                }),
                Err(_) => Err(CollectionError {
                    service,
                    container,
                    error: format!(
                        "log collection timed out after {}s",
                        SOURCE_TIMEOUT.as_secs()
                    ),
                }),
            }
        }
    }))
    .buffer_unordered(MAX_CONCURRENT_SOURCES)
    .collect::<Vec<_>>()
    .await;

    for result in results {
        match result {
            Ok(source) => report.sources.push(source),
            Err(error) => report.collection_errors.push(error),
        }
    }
    report.sources.sort_by(|left, right| {
        left.service
            .cmp(&right.service)
            .then_with(|| left.container.cmp(&right.container))
    });
    report.collection_errors.sort_by(|left, right| {
        left.service
            .cmp(&right.service)
            .then_with(|| left.container.cmp(&right.container))
    });
    report
}

fn empty_export() -> ProcessLogExport {
    ProcessLogExport {
        format: "myriad-process-log-export",
        schema_version: 1,
        generated_at: Utc::now(),
        scope: "core-compose-services",
        limits: ExportLimits {
            scanned_lines_per_source: MAX_SCANNED_LINES_PER_SOURCE,
            retained_entries_per_source: MAX_ENTRIES_PER_SOURCE,
            input_line_bytes: MAX_INPUT_LINE_BYTES,
            entry_bytes: MAX_ENTRY_BYTES,
            source_response_bytes: MAX_SOURCE_RESPONSE_BYTES,
        },
        sources: Vec::new(),
        collection_errors: Vec::new(),
        omitted_sources: Vec::new(),
    }
}

/// Tails tried in order when the guard rejects a log window as larger than its
/// response bound. Recent lines matter most, so a shorter window beats losing
/// the whole source to a burst of oversized lines.
const SOURCE_TAIL_STEPS: [u64; 3] = [MAX_CONTAINER_LOG_TAIL, 2_000, 400];

enum ReadError {
    TooLarge,
    Failed(String),
}

async fn collect_source(
    docker: Arc<DockerClient>,
    source: ContainerSource,
) -> Result<ProcessLogSource, String> {
    for (step, tail) in SOURCE_TAIL_STEPS.into_iter().enumerate() {
        match read_source(&docker, &source, tail).await {
            Ok(mut collected) => {
                if step > 0 {
                    collected.truncated = true;
                    collected.complete = false;
                }
                return Ok(collected);
            }
            Err(ReadError::TooLarge) if step + 1 < SOURCE_TAIL_STEPS.len() => continue,
            Err(ReadError::TooLarge) => {
                return Err(format!(
                    "read logs: the last {tail} lines exceed {MAX_SOURCE_RESPONSE_BYTES} bytes"
                ));
            }
            Err(ReadError::Failed(error)) => return Err(error),
        }
    }
    unreachable!("the last tail step returns")
}

async fn read_source(
    docker: &DockerClient,
    source: &ContainerSource,
    tail: u64,
) -> Result<ProcessLogSource, ReadError> {
    let tail_lines = tail.to_string();
    let options = LogsOptionsBuilder::default()
        .stdout(true)
        .stderr(true)
        .timestamps(true)
        .tail(&tail_lines)
        .build();
    let mut logs = docker.raw().logs(&source.id, Some(options));
    let mut entries = VecDeque::new();
    let mut scanned_lines = 0;
    let mut matched_lines = 0;
    let mut input_truncated = false;

    while let Some(output) = logs.next().await {
        let output = output.map_err(|error| match error {
            bollard::errors::Error::DockerResponseServerError {
                status_code: 413, ..
            } => ReadError::TooLarge,
            error => ReadError::Failed(format!("read logs: {error}")),
        })?;
        let (stream, bytes) = match output {
            LogOutput::StdOut { message } => (LogStream::Stdout, message),
            LogOutput::StdErr { message } => (LogStream::Stderr, message),
            LogOutput::Console { message } => (LogStream::Console, message),
            LogOutput::StdIn { .. } => continue,
        };
        for raw_line in log_lines(&bytes) {
            scanned_lines += 1;
            let raw_line = if raw_line.len() > MAX_INPUT_LINE_BYTES {
                input_truncated = true;
                &raw_line[..MAX_INPUT_LINE_BYTES]
            } else {
                raw_line
            };
            let raw_line = String::from_utf8_lossy(raw_line);
            let line = normalize_line(&raw_line);
            if line.is_empty() {
                continue;
            }
            let (timestamp, message) = split_timestamp(&line);
            let Some(level) = classify_line(message, stream) else {
                continue;
            };
            matched_lines += 1;
            let sanitized = sanitize_message(message);
            let (message, truncated) = truncate_utf8(&sanitized, MAX_ENTRY_BYTES);
            if entries.len() >= MAX_ENTRIES_PER_SOURCE {
                entries.pop_front();
            }
            entries.push_back(ProcessLogEntry {
                timestamp,
                stream,
                level,
                message,
                truncated,
            });
        }
    }

    let scan_limited = scanned_lines >= tail as usize;
    let entry_limited = matched_lines > entries.len();
    Ok(ProcessLogSource {
        service: source.service.clone(),
        container: source.container.clone(),
        state: source.state.clone(),
        entries: entries.into(),
        scanned_lines,
        matched_lines,
        input_truncated,
        truncated: scan_limited || entry_limited || input_truncated,
        complete: !scan_limited && !entry_limited && !input_truncated,
    })
}

fn log_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    if bytes.ends_with(b"\n") {
        lines.pop();
    }
    lines
}

fn normalize_line(line: &str) -> String {
    let without_ansi = ANSI_ESCAPE.replace_all(line, "");
    without_ansi
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect::<String>()
        .trim()
        .to_string()
}

fn split_timestamp(line: &str) -> (Option<String>, &str) {
    let Some((candidate, message)) = line.split_once(' ') else {
        return (None, line);
    };
    if DateTime::parse_from_rfc3339(candidate).is_ok() {
        (Some(candidate.to_string()), message.trim_start())
    } else {
        (None, line)
    }
}

fn classify_line(line: &str, stream: LogStream) -> Option<ExportLevel> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("panicked at") || lower.starts_with("panic:") {
        return Some(ExportLevel::Error);
    }
    if let Some(captures) = PREFIX_LEVEL
        .captures(line)
        .or_else(|| STRUCTURED_LEVEL.captures(line))
    {
        return match captures
            .name("level")
            .map(|level| level.as_str().to_ascii_lowercase())
            .as_deref()
        {
            Some("warn" | "warning") => Some(ExportLevel::Warning),
            Some("error" | "fatal" | "panic") => Some(ExportLevel::Error),
            _ => None,
        };
    }
    match stream {
        LogStream::Stderr => Some(ExportLevel::UnclassifiedStderr),
        LogStream::Stdout | LogStream::Console => None,
    }
}

fn sanitize_message(message: &str) -> String {
    let redacted = redact_secrets(message);
    let redacted = JSON_SECRET.replace_all(&redacted, "${prefix}[REDACTED]");
    let redacted = CREDENTIAL_URL.replace_all(&redacted, "${prefix}[REDACTED]@");
    let redacted = URL_QUERY_SECRET.replace_all(&redacted, "${prefix}[REDACTED]");
    JWT.replace_all(&redacted, "[JWT_REDACTED]").into_owned()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let suffix = "...";
    if max_bytes <= suffix.len() {
        return (suffix[..max_bytes].to_string(), true);
    }
    let mut end = max_bytes - suffix.len();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}{}", &value[..end], suffix), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A daemon behind the guard: windows over `max_tail` lines exceed the log bound.
    async fn log_daemon(max_tail: u64) -> Arc<DockerClient> {
        use axum::response::IntoResponse;
        use std::future::IntoFuture;
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri| async move {
            let tail = uri
                .query()
                .unwrap_or_default()
                .split('&')
                .find_map(|pair| pair.strip_prefix("tail="))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            if tail > max_tail {
                return (
                    axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                    axum::Json(serde_json::json!({"message": "Docker log response exceeds 4 MiB"})),
                )
                    .into_response();
            }
            let mut body = Vec::new();
            for line in 0..tail {
                let payload = format!("2026-09-24T00:00:00Z WARN line {line}\n");
                body.extend_from_slice(&[1, 0, 0, 0]);
                body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                body.extend_from_slice(payload.as_bytes());
            }
            body.into_response()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(axum::serve(listener, app).into_future());
        let docker =
            bollard::Docker::connect_with_http(&address, 5, bollard::API_DEFAULT_VERSION).unwrap();
        Arc::new(DockerClient::from_raw(docker))
    }

    fn backend_source() -> ContainerSource {
        ContainerSource {
            id: "backend-id".into(),
            service: "backend".into(),
            container: "myriad-backend".into(),
            state: Some("running".into()),
            running: true,
            created: 0,
        }
    }

    #[tokio::test]
    async fn oversized_log_window_falls_back_to_recent_lines() {
        let source = collect_source(log_daemon(2_000).await, backend_source())
            .await
            .unwrap();
        assert_eq!(source.scanned_lines, 2_000);
        assert_eq!(source.entries.len(), MAX_ENTRIES_PER_SOURCE);
        assert_eq!(source.entries.last().unwrap().message, "WARN line 1999");
        assert!(source.truncated);
        assert!(!source.complete);
    }

    #[tokio::test]
    async fn a_window_oversized_at_every_step_reports_the_bound() {
        let error = collect_source(log_daemon(0).await, backend_source())
            .await
            .unwrap_err();
        assert!(error.contains("the last 400 lines exceed"), "{error}");
    }

    #[test]
    fn classifies_explicit_levels_without_promoting_info_messages() {
        assert!(matches!(
            classify_line("2026-01-01T00:00:00Z WARN retrying", LogStream::Stdout),
            Some(ExportLevel::Warning)
        ));
        assert!(matches!(
            classify_line("ERROR: relation missing", LogStream::Stderr),
            Some(ExportLevel::Error)
        ));
        assert!(matches!(
            classify_line(r#"{"level":"error","message":"failed"}"#, LogStream::Stdout),
            Some(ExportLevel::Error)
        ));
        assert!(matches!(
            classify_line("level=warning operation delayed", LogStream::Stdout),
            Some(ExportLevel::Warning)
        ));
        assert!(classify_line("INFO no error was found", LogStream::Stdout).is_none());
        assert!(classify_line("no error was found", LogStream::Stdout).is_none());
        assert!(classify_line("INFO experience_level=error", LogStream::Stdout).is_none());
        assert!(classify_line("INFO level=error", LogStream::Stdout).is_none());
        assert!(matches!(
            classify_line("request handler failed", LogStream::Stderr),
            Some(ExportLevel::UnclassifiedStderr)
        ));
    }

    #[test]
    fn strips_timestamp_ansi_and_control_characters() {
        let line = normalize_line("\u{1b}[31m2026-01-01T00:00:00Z ERROR bad\u{0}\u{1b}[0m");
        let (timestamp, message) = split_timestamp(&line);
        assert_eq!(timestamp.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(message, "ERROR bad");
    }

    #[test]
    fn redacts_structured_credentials_and_database_urls() {
        let line = r#"error {"access_token":"secret-value"} https://api.example.test/data?key=steam-secret&lang=en postgres://user:pass@db/app eyJabcdefgh.abcdefgh.abcdefgh"#;
        let sanitized = sanitize_message(line);
        assert!(!sanitized.contains("secret-value"));
        assert!(!sanitized.contains("steam-secret"));
        assert!(!sanitized.contains(":pass@"));
        assert!(!sanitized.contains("eyJabcdefgh"));
        assert!(sanitized.contains("[REDACTED]"));
    }

    #[test]
    fn truncates_only_at_utf8_boundaries() {
        assert_eq!(truncate_utf8("abc", 3), ("abc".into(), false));
        assert_eq!(truncate_utf8("a界b", 4), ("a...".into(), true));
        assert_eq!(truncate_utf8("abcdef", 3), ("...".into(), true));
    }

    #[test]
    fn trailing_newline_does_not_create_an_extra_scanned_line() {
        assert_eq!(log_lines(b"ERROR one\n").len(), 1);
        assert_eq!(log_lines(b"ERROR one\nWARN two\n").len(), 2);
        assert_eq!(log_lines(b"\n").len(), 1);
        assert_eq!(log_lines(b"ERROR one\n\n").len(), 2);
    }
}
