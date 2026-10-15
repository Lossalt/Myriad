//! 跑 Hugo，再对输出做后处理。子进程环境只留 PATH、HOME、TMPDIR 和缓存目录。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use myriad_blog::{
    BlogPost, BlogSettings, BlogSite, FriendLink, NOTES_CSS, export_site_files, is_script_asset,
    postprocess_html,
};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::layout::BlogLayout;
use super::themes::{copy_theme_into, ensure_default_theme};

const HUGO_TIMEOUT: Duration = Duration::from_secs(120);
const LOG_TAIL_CHARS: usize = 4_000;
const HTML_BYTE_CAP: usize = 20 * 1024 * 1024;

pub struct BuiltSite {
    pub log_tail: String,
    pub hugo_version: Option<String>,
}

#[derive(Debug)]
pub enum BuildProblem {
    Unavailable,
    Failed {
        message: &'static str,
        log_tail: String,
    },
}

pub async fn hugo_version_line() -> Option<String> {
    let child = hugo_command(&[])
        .arg("version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let finished = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await;
    let output = finished.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| !line.trim().is_empty())?;
    Some(scrub_log(line).chars().take(200).collect())
}

pub async fn render_and_build(
    layout: &BlogLayout,
    site: &BlogSite,
    settings: &BlogSettings,
    posts: &[BlogPost],
    friends: &[FriendLink],
) -> Result<BuiltSite, BuildProblem> {
    ensure_default_theme(layout).map_err(|_| BuildProblem::Failed {
        message: "Could not install the default theme",
        log_tail: String::new(),
    })?;
    let theme_id = myriad_blog::sanitize_theme_id(&settings.theme_id)
        .unwrap_or_else(|| myriad_blog::DEFAULT_THEME_ID.to_string());
    if !layout.theme_dir(&theme_id).join("theme.toml").is_file()
        && theme_id != myriad_blog::DEFAULT_THEME_ID
    {
        return Err(BuildProblem::Failed {
            message: "Selected theme is not installed",
            log_tail: String::new(),
        });
    }

    let work = layout.work_dir();
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|_| failed("Could not prepare the build", ""))?;
    }
    let site_dir = work.join("site");
    let home = work.join("home");
    let tmp = work.join("tmp");
    let public = site_dir.join("public");
    for dir in [&site_dir, &home, &tmp, &layout.hugo_cache] {
        fs::create_dir_all(dir).map_err(|_| failed("Could not prepare the build", ""))?;
    }
    for (rel, body) in export_site_files(site, settings, posts, friends) {
        let path = site_dir.join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| failed("Could not prepare the build", ""))?;
        }
        fs::write(path, body).map_err(|_| failed("Could not prepare the build", ""))?;
    }
    copy_theme_into(layout, &theme_id, &site_dir.join("themes").join(&theme_id))
        .map_err(|_| failed("Could not prepare the theme", ""))?;

    let log = match run_hugo(&site_dir, &public, &home, &tmp, &layout.hugo_cache).await {
        Ok(log) => log,
        Err(problem) => {
            let _ = fs::remove_dir_all(&work);
            return Err(problem);
        }
    };
    if let Err(message) = postprocess_output(&public) {
        let _ = fs::remove_dir_all(&work);
        return Err(failed(message, &log));
    }
    super::layout::publish_tree(&layout.current_dir(), &public).map_err(|message| {
        let _ = fs::remove_dir_all(&work);
        failed(message, &log)
    })?;
    let _ = fs::remove_dir_all(&work);
    let version = hugo_version_line().await;
    Ok(BuiltSite {
        log_tail: log,
        hugo_version: version,
    })
}

async fn run_hugo(
    site: &Path,
    public: &Path,
    home: &Path,
    tmp: &Path,
    cache: &Path,
) -> Result<String, BuildProblem> {
    let mut child = hugo_command(&[("HOME", home), ("TMPDIR", tmp), ("HUGO_CACHEDIR", cache)])
        .current_dir(site)
        .arg("build")
        .arg("--source")
        .arg(site)
        .arg("--destination")
        .arg(public)
        .arg("--cacheDir")
        .arg(cache)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BuildProblem::Unavailable
            } else {
                failed("Could not start Hugo", "")
            }
        })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = tokio::spawn(async move { read_capped(stdout).await });
    let stderr_task = tokio::spawn(async move { read_capped(stderr).await });
    let waited = tokio::time::timeout(HUGO_TIMEOUT, child.wait()).await;
    let (status, timed_out) = match waited {
        Ok(Ok(status)) => (Some(status), false),
        Ok(Err(_)) => (None, false),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            (None, true)
        }
    };
    let stdout = stdout_task.await.unwrap_or_default();
    let stderr = stderr_task.await.unwrap_or_default();
    let log = tail(&scrub_log(&format!("{stdout}{stderr}")));
    let Some(status) = status else {
        return Err(failed(
            if timed_out {
                "Hugo timed out"
            } else {
                "Hugo did not finish"
            },
            &log,
        ));
    };
    if !status.success() {
        return Err(failed("Hugo build failed", &log));
    }
    Ok(log)
}

fn hugo_command(extra: &[(&str, &Path)]) -> Command {
    let bin = std::env::var("HUGO_BIN").unwrap_or_else(|_| "hugo".to_string());
    let bin = if bin.trim().is_empty() {
        "hugo".to_string()
    } else {
        bin
    };
    let mut command = Command::new(bin);
    command.env_clear().env("PATH", hugo_path());
    for (key, value) in extra {
        command.env(key, value);
    }
    command
}

fn hugo_path() -> String {
    match std::env::var("DART_SASS_DIR") {
        Ok(dir) if !dir.trim().is_empty() => {
            format!("{}:/usr/bin:/bin", dir.trim().trim_end_matches('/'))
        }
        _ => "/usr/bin:/bin".to_string(),
    }
}

async fn read_capped<R>(pipe: Option<R>) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8 * 1024];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if buf.len() < 64 * 1024 {
                    let room = 64 * 1024 - buf.len();
                    buf.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn postprocess_output(public: &Path) -> Result<(), &'static str> {
    let mut files = Vec::new();
    collect_files(public, public, &mut files)?;
    for path in files {
        let rel = path
            .strip_prefix(public)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_script_asset(&rel) {
            fs::remove_file(&path).map_err(|_| "Could not clean the built site")?;
            continue;
        }
        let kind = path
            .symlink_metadata()
            .map_err(|_| "Could not clean the built site")?;
        if kind.file_type().is_symlink() {
            fs::remove_file(&path).map_err(|_| "Could not clean the built site")?;
            continue;
        }
        if !is_html(&path) {
            continue;
        }
        let bytes = fs::read(&path).map_err(|_| "Could not clean the built site")?;
        if bytes.len() > HTML_BYTE_CAP {
            return Err("A generated page is too large");
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let cleaned = postprocess_html(&text);
        fs::write(&path, cleaned).map_err(|_| "Could not clean the built site")?;
    }
    let css = public.join("_myriad").join("notes.css");
    if let Some(parent) = css.parent() {
        fs::create_dir_all(parent).map_err(|_| "Could not clean the built site")?;
    }
    fs::write(css, NOTES_CSS).map_err(|_| "Could not clean the built site")?;
    Ok(())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), &'static str> {
    let entries = fs::read_dir(dir).map_err(|_| "Could not clean the built site")?;
    for entry in entries {
        let entry = entry.map_err(|_| "Could not clean the built site")?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| "Could not clean the built site")?;
        if meta.file_type().is_symlink() {
            out.push(path);
            continue;
        }
        if meta.is_dir() {
            if path.starts_with(root) {
                collect_files(root, &path, out)?;
            }
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn is_html(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "html" | "htm"
    )
}

fn failed(message: &'static str, log: &str) -> BuildProblem {
    BuildProblem::Failed {
        message,
        log_tail: tail(&scrub_log(log)),
    }
}

fn tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= LOG_TAIL_CHARS {
        return text.to_string();
    }
    text.chars().skip(count - LOG_TAIL_CHARS).collect()
}

/// 日志里如果出现 `SECRET=...` 这种赋值，把值抹掉。路径可以留着。
pub fn scrub_log(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if let Some(index) = line.find('=') {
            let key = line[..index].trim();
            let lower = key.to_ascii_lowercase();
            if lower.contains("secret")
                || lower.contains("password")
                || lower.contains("token")
                || lower.contains("api_key")
                || lower.contains("database_url")
            {
                out.push_str(key);
                out.push_str("=[redacted]");
                if line.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubber_redacts_assignments_and_keeps_ordinary_lines() {
        let cleaned =
            scrub_log("build ok\nDATABASE_URL=postgres://user:secret@db/app\nPATH=/bin\n");
        assert!(cleaned.contains("build ok"));
        assert!(cleaned.contains("DATABASE_URL=[redacted]"));
        assert!(!cleaned.contains("postgres://"));
        assert!(cleaned.contains("PATH=/bin"));
    }
}
