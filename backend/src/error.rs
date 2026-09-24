//! Axum adapter for the shared [`myriad_error::AppError`].
//!
//! Domain / services depend only on `myriad-error`. This module is the sole place
//! that couples that type to Axum's `IntoResponse`.
//!
//! We use a local newtype [`HttpError`] because Rust's orphan rules forbid
//! `impl IntoResponse for AppError` (both the trait and the type are foreign).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use myriad_error::AppError;

/// Local wrapper so handlers can `return Err(HttpError(...))` / `.into_response()`.
#[derive(Debug)]
pub struct HttpError(pub AppError);

impl From<AppError> for HttpError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl From<HttpError> for AppError {
    fn from(err: HttpError) -> Self {
        err.0
    }
}

/// Convert a shared [`AppError`] into an Axum response.
pub fn app_error_response(err: AppError) -> Response {
    let status =
        StatusCode::from_u16(err.status_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(err.to_json())).into_response()
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        app_error_response(self.0)
    }
}

/// Bridge `(StatusCode, Json<Value>)` handler errors into [`HttpError`].
pub fn status_json_to_http(err: (StatusCode, axum::Json<serde_json::Value>)) -> HttpError {
    let (status, axum::Json(v)) = err;
    let label = v
        .get("error")
        .and_then(|x| x.as_str())
        .unwrap_or("error")
        .to_string();
    let mut app = AppError::from_status_u16(status.as_u16(), label);
    if let Some(m) = v.get("message").and_then(|x| x.as_str()) {
        app = app.with_message(m);
    }
    if let Some(h) = v.get("hint").and_then(|x| x.as_str()) {
        app = app.with_hint(h);
    }
    if let Some(c) = v.get("code").and_then(|x| x.as_str()) {
        app = app.with_code(c);
    }
    HttpError(app)
}

/// Allows `?` on legacy `(StatusCode, Json<_>)` errors inside `Result<_, HttpError>` handlers.
impl From<(StatusCode, axum::Json<serde_json::Value>)> for HttpError {
    fn from(err: (StatusCode, axum::Json<serde_json::Value>)) -> Self {
        status_json_to_http(err)
    }
}

/// Bridge bare `StatusCode` handler errors (SEO helpers; not platform-proxy).
impl From<StatusCode> for HttpError {
    fn from(status: StatusCode) -> Self {
        let label = status.canonical_reason().unwrap_or("error").to_string();
        HttpError(AppError::from_status_u16(status.as_u16(), label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn into_response_uses_status_and_json_error_field() {
        let resp = HttpError(AppError::conflict("Admin account already exists")).into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Admin account already exists");
    }

    #[tokio::test]
    async fn response_redacts_secrets_in_message() {
        let resp = HttpError(
            AppError::bad_gateway("upstream")
                .with_message("Authorization: Bearer supersecrettoken99"),
        )
        .into_response();
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let s = String::from_utf8_lossy(&bytes);
        assert!(!s.contains("supersecrettoken99"), "leaked: {s}");
        assert!(s.contains("[REDACTED]"), "got: {s}");
    }

    #[tokio::test]
    async fn app_error_response_matches_http_error() {
        let err = AppError::not_found("gone");
        let a = app_error_response(err.clone());
        let b = HttpError(err).into_response();
        assert_eq!(a.status(), b.status());
        assert_eq!(a.status(), StatusCode::NOT_FOUND);
    }

    /// Federation write handlers bridge legacy domain `(StatusCode, Json)` via
    /// [`status_json_to_http`] — this drives that real path.
    #[tokio::test]
    async fn status_json_to_http_preserves_status_label_and_hint() {
        let err = status_json_to_http((
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "Key rotation requires confirm",
                "hint": "pass {\"confirm\": true}",
                "message": "rotation aborted",
            })),
        ));
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Key rotation requires confirm");
        assert_eq!(v["hint"], "pass {\"confirm\": true}");
        assert_eq!(v["message"], "rotation aborted");
    }

    #[tokio::test]
    async fn status_json_to_http_preserves_machine_code() {
        let err = status_json_to_http((
            StatusCode::BAD_GATEWAY,
            axum::Json(serde_json::json!({
                "error": "Failed to suggest a name",
                "code": "name_suggest_failed",
                "message": "provider timed out",
            })),
        ));
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Failed to suggest a name");
        assert_eq!(v["code"], "name_suggest_failed");
        assert_eq!(v["message"], "provider timed out");
    }

    #[tokio::test]
    async fn status_json_to_http_preserves_error_field_for_write_paths() {
        // Write-path bridge used by setup/config: legacy (StatusCode, Json)
        // errors become HttpError with the same public `error`.
        let legacy = (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": "Setup already completed",
                "message": "Admin exists"
            })),
        );
        let resp = status_json_to_http(legacy).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Setup already completed");
        assert_eq!(v["message"], "Admin exists");
    }

    /// Rust sources that ship: `backend/src` and `crates/*/src`, minus
    /// integration-test directories, out-of-line `#[cfg(test)]` modules and
    /// inline `#[cfg(test)]` items. Test fixtures are not client-facing.
    fn production_sources() -> &'static [String] {
        static SOURCES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        SOURCES.get_or_init(load_production_sources)
    }

    fn load_production_sources() -> Vec<String> {
        use std::path::{Path, PathBuf};
        fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    let skip = path.file_name().is_some_and(|name| {
                        ["target", "tests", "benches", "examples"]
                            .iter()
                            .any(|s| name == *s)
                    });
                    if !skip {
                        collect(&path, out);
                    }
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect(&root.join("src"), &mut files);
        collect(&root.join("../crates"), &mut files);
        let sources: Vec<(PathBuf, String)> = files
            .into_iter()
            .map(|path| {
                let source = std::fs::read_to_string(&path).unwrap();
                (path, source)
            })
            .collect();
        // `#[cfg(test)] mod name;` names a file `name.rs` or a directory `name/`.
        let mut test_modules: Vec<PathBuf> = Vec::new();
        for (path, source) in &sources {
            let parent = path.parent().unwrap();
            let stem = path.file_stem().unwrap().to_str().unwrap();
            let base = if ["mod", "lib", "main"].contains(&stem) {
                parent.to_path_buf()
            } else {
                parent.join(stem)
            };
            for rest in source.split("#[cfg(test)]").skip(1) {
                let mut rest = rest.trim_start();
                while rest.starts_with("#[") {
                    rest = rest[rest.find(']').unwrap() + 1..].trim_start();
                }
                let Some(decl) = rest
                    .strip_prefix("pub(crate) mod ")
                    .or_else(|| rest.strip_prefix("pub mod "))
                    .or_else(|| rest.strip_prefix("mod "))
                else {
                    continue;
                };
                let name: String = decl
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if decl[name.len()..].trim_start().starts_with(';') {
                    test_modules.push(base.join(&name));
                }
            }
        }
        sources
            .into_iter()
            .filter(|(path, _)| {
                !test_modules
                    .iter()
                    .any(|module| path.starts_with(module) || *path == module.with_extension("rs"))
            })
            .map(|(_, source)| strip_cfg_test_items(&source))
            .collect()
    }

    /// Drop every item annotated `#[cfg(test)]` (block or `;`-terminated).
    fn strip_cfg_test_items(source: &str) -> String {
        const MARK: &str = "#[cfg(test)]";
        let mut out = String::with_capacity(source.len());
        let mut rest = source;
        while let Some(at) = rest.find(MARK) {
            out.push_str(&rest[..at]);
            rest = skip_item(&rest[at + MARK.len()..]);
        }
        out.push_str(rest);
        out
    }

    /// Skip one item: through its balanced `{…}` body, or through `;` when the
    /// item has no body. Braces inside strings, chars and comments do not count.
    fn skip_item(src: &str) -> &str {
        let bytes = src.as_bytes();
        let mut depth = 0usize;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                b'/' if bytes.get(i + 1) == Some(&b'*') => {
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    i += 1;
                }
                b'r' if matches!(bytes.get(i + 1), Some(b'"' | b'#'))
                    && (i == 0
                        || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) =>
                {
                    let hashes = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                    if bytes.get(i + 1 + hashes) == Some(&b'"') {
                        let close = format!("\"{}", "#".repeat(hashes));
                        let body = i + 2 + hashes;
                        i = body + src[body..].find(&close).unwrap() + close.len() - 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'\'' => {
                    if bytes.get(i + 1) == Some(&b'\\') {
                        i += 2;
                        while i < bytes.len() && bytes[i] != b'\'' {
                            i += 1;
                        }
                    } else if bytes.get(i + 2) == Some(&b'\'') {
                        i += 2;
                    }
                    // Otherwise a lifetime: nothing to skip.
                }
                b'{' => depth += 1,
                // A `#[cfg(test)]` field or arm: stop at the enclosing close.
                b'}' if depth == 0 => return &src[i..],
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &src[i + 1..];
                    }
                }
                b';' if depth == 0 => return &src[i + 1..],
                _ => {}
            }
            i += 1;
        }
        ""
    }

    /// The string literal that `rest` opens with (after `"`), escapes kept as
    /// written, and the text after its closing quote.
    fn read_literal(rest: &str) -> (String, &str) {
        let mut literal = String::new();
        let mut chars = rest.char_indices();
        while let Some((at, c)) = chars.next() {
            match c {
                '"' => return (literal, &rest[at + 1..]),
                '\\' => {
                    literal.push('\\');
                    if let Some((_, next)) = chars.next() {
                        literal.push(next);
                    }
                }
                c => literal.push(c),
            }
        }
        (literal, "")
    }

    /// String literals that follow `needle` in shipped code.
    fn literals_after(needle: &str) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        for source in production_sources() {
            for rest in source.split(needle).skip(1) {
                if let Some(rest) = rest.trim_start().strip_prefix('"') {
                    out.insert(read_literal(rest).0);
                }
            }
        }
        out
    }

    /// String literals returned by `fn code(..) -> &'static str` / `fn *_code(..)`,
    /// the error-enum accessors whose result is passed to `.with_code(..)`.
    fn code_fn_literals() -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        for source in production_sources() {
            for rest in source.split("fn ").skip(1) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if name != "code" && !name.ends_with("_code") {
                    continue;
                }
                let Some(open) = rest.find('{') else { continue };
                if !rest[..open].contains("-> &'static str") {
                    continue;
                }
                let body = &rest[open..];
                let mut body = &body[..body.len() - skip_item(body).len()];
                while let Some(at) = body.find('"') {
                    // Only returned literals, not text the function matches on.
                    let before = body[..at].trim_end();
                    let (literal, after) = read_literal(&body[at + 1..]);
                    if before.ends_with("=>") || before.ends_with("return") {
                        out.insert(literal);
                    }
                    body = after;
                }
            }
        }
        out
    }

    /// Public error labels written in code, as the scanner reads them.
    fn literal_labels() -> std::collections::BTreeSet<String> {
        const CONSTRUCTORS: &[&str] = &[
            "bad_request",
            "unauthorized",
            "forbidden",
            "not_found",
            "conflict",
            "service_unavailable",
            "bad_gateway",
            "internal",
            "public_json",
            "fail_json",
        ];
        CONSTRUCTORS
            .iter()
            .flat_map(|constructor| literals_after(&format!("AppError::{constructor}(")))
            .collect()
    }

    /// Error codes written directly in shipped code rather than inferred from a label.
    fn literal_codes() -> std::collections::BTreeSet<String> {
        let mut out = literals_after(".with_code(");
        out.extend(literals_after("\"code\":"));
        out.extend(code_fn_literals());
        out.retain(|code| {
            code.starts_with(|c: char| c.is_ascii_alphabetic())
                && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        out
    }

    /// Clients localize errors by code. A new public label must come with a
    /// code in `shared/error_codes.json`; labels from before this rule are
    /// listed in `shared/error_labels_uncoded.txt`, which may only shrink.
    #[test]
    fn every_public_error_label_has_a_code() {
        let uncoded: std::collections::BTreeSet<&str> =
            include_str!("../../shared/error_labels_uncoded.txt")
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect();
        let labels = literal_labels();
        let missing: Vec<_> = labels
            .iter()
            .filter(|label| myriad_error::AppError::inferred_code(label).is_none())
            .filter(|label| !uncoded.contains(label.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "give these labels a code in shared/error_codes.json: {missing:#?}"
        );
        let stale: Vec<_> = uncoded
            .iter()
            .filter(|label| {
                !labels.contains(**label) || myriad_error::AppError::inferred_code(label).is_some()
            })
            .collect();
        assert!(
            stale.is_empty(),
            "remove from shared/error_labels_uncoded.txt: {stale:#?}"
        );
    }

    /// A code set directly (`.with_code("…")`, `"code": "…"`) must be listed in
    /// `shared/error_codes.json` (`labels` values or `explicit`) so clients can
    /// give it copy; an unregistered code reaches users as bare text.
    #[test]
    fn every_explicit_error_code_is_registered() {
        let spec: serde_json::Value =
            serde_json::from_str(include_str!("../../shared/error_codes.json")).unwrap();
        let mut known: std::collections::BTreeSet<&str> = spec["labels"]
            .as_object()
            .unwrap()
            .values()
            .filter_map(|code| code.as_str())
            .collect();
        known.extend(
            spec["explicit"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|code| code.as_str()),
        );
        let missing: Vec<_> = literal_codes()
            .into_iter()
            .filter(|code| !known.contains(code.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "register these codes in shared/error_codes.json `explicit`: {missing:#?}"
        );
    }
}
