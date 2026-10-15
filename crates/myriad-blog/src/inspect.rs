//! 主题包识别。解压由后端做；这里判断路径是否安全，并读出名字和最低 Hugo 版本。

use std::fmt;
use std::path::Path;

use crate::archive::safe_archive_path;

/// 主题包最多收多少个文件。
pub const MAX_THEME_FILES: usize = 10_000;

/// 主题包解压后的总字节数上限。
pub const MAX_THEME_BYTES: u64 = 64 * 1024 * 1024;

/// 一个已解压条目。`body` 只在 `theme.toml` / `hugo.toml` / `config.toml` 上需要。
pub struct ThemeFile<'a> {
    pub path: &'a str,
    pub size: u64,
    pub body: Option<&'a str>,
}

/// 认出来的主题。`id` 是小写 `[a-z0-9-]`。`root` 相对压缩包根，包根本身是主题时为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub min_version: Option<String>,
    pub root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeInspectError {
    UnsafePath { path: String },
    TooManyFiles { count: usize, limit: usize },
    TooLarge { bytes: u64, limit: u64 },
    NotATheme,
    InvalidId,
}

impl fmt::Display for ThemeInspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafePath { path } => write!(f, "主题包路径不安全：{path}"),
            Self::TooManyFiles { count, limit } => {
                write!(f, "主题包文件过多：{count} 个，上限 {limit}")
            }
            Self::TooLarge { bytes, limit } => write!(f, "主题包过大：{bytes} 字节，上限 {limit}"),
            Self::NotATheme => write!(f, "压缩包里没有 Hugo 主题"),
            Self::InvalidId => write!(f, "主题名不能转成 id"),
        }
    }
}

impl std::error::Error for ThemeInspectError {}

struct Marker {
    dir: String,
    rank: u8,
    junk: bool,
    depth: usize,
}

/// 找到主题根，读出 name / min_version（含 `[module.hugoVersion] min`），并拒绝危险路径和过大的包。
pub fn inspect_theme(files: &[ThemeFile<'_>]) -> Result<ThemeInfo, ThemeInspectError> {
    let mut entries = Vec::with_capacity(files.len());
    let mut total = 0u64;
    for file in files {
        let Some(path) = safe_archive_path(file.path) else {
            return Err(ThemeInspectError::UnsafePath {
                path: file.path.to_string(),
            });
        };
        let path = path_text(&path);
        if path.is_empty() {
            return Err(ThemeInspectError::UnsafePath {
                path: file.path.to_string(),
            });
        }
        total = total.saturating_add(file.size);
        entries.push((path, file.body));
    }
    if entries.len() > MAX_THEME_FILES {
        return Err(ThemeInspectError::TooManyFiles {
            count: entries.len(),
            limit: MAX_THEME_FILES,
        });
    }
    if total > MAX_THEME_BYTES {
        return Err(ThemeInspectError::TooLarge {
            bytes: total,
            limit: MAX_THEME_BYTES,
        });
    }

    let root = theme_root(&entries).ok_or(ThemeInspectError::NotATheme)?;
    let parsed = ["theme.toml", "hugo.toml", "config.toml"]
        .into_iter()
        .filter_map(|name| config_body(&entries, &root, name))
        .filter_map(|body| {
            toml::from_str::<toml::Table>(body)
                .ok()
                .map(toml::Value::Table)
        })
        .collect::<Vec<_>>();
    let name = parsed
        .iter()
        .find_map(|value| top_string(value, "name"))
        .or_else(|| directory_name(&root).map(str::to_string))
        .ok_or(ThemeInspectError::InvalidId)?;
    let version = parsed.iter().find_map(|value| top_string(value, "version"));
    let min_version = parsed
        .iter()
        .find_map(|value| top_string(value, "min_version").or_else(|| hugo_version_min(value)));
    let id = normalize_theme_id(&name).ok_or(ThemeInspectError::InvalidId)?;
    Ok(ThemeInfo {
        id,
        name,
        version,
        min_version,
        root,
    })
}

fn theme_root(entries: &[(String, Option<&str>)]) -> Option<String> {
    let mut markers = Vec::new();
    for (path, _) in entries {
        let name = file_name(path);
        let parent = parent_dir(path);
        if matches!(name, "theme.toml" | "hugo.toml" | "config.toml") {
            let rank = if name == "theme.toml" { 0 } else { 1 };
            push_marker(&mut markers, parent, rank);
        }
        if let Some(owner) = layouts_owner(path) {
            push_marker(&mut markers, owner, 2);
        }
    }
    if markers.iter().any(|marker| !marker.junk) {
        markers.retain(|marker| !marker.junk);
    }
    markers.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then(left.depth.cmp(&right.depth))
            .then(left.dir.cmp(&right.dir))
    });
    markers.into_iter().next().map(|marker| marker.dir)
}

fn push_marker(markers: &mut Vec<Marker>, dir: String, rank: u8) {
    if let Some(existing) = markers.iter_mut().find(|marker| marker.dir == dir) {
        if rank < existing.rank {
            existing.rank = rank;
        }
        return;
    }
    let depth = if dir.is_empty() {
        0
    } else {
        dir.split('/').count()
    };
    let junk = dir
        .split('/')
        .any(|part| part == "__MACOSX" || part == ".git");
    markers.push(Marker {
        dir,
        rank,
        junk,
        depth,
    });
}

fn layouts_owner(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    let index = parts.iter().position(|part| *part == "layouts")?;
    if index + 1 >= parts.len() {
        return None;
    }
    Some(parts[..index].join("/"))
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn directory_name(dir: &str) -> Option<&str> {
    if dir.is_empty() {
        None
    } else {
        dir.rsplit('/').next()
    }
}

fn config_body<'a>(
    entries: &'a [(String, Option<&str>)],
    root: &str,
    name: &str,
) -> Option<&'a str> {
    let wanted = if root.is_empty() {
        name.to_string()
    } else {
        format!("{root}/{name}")
    };
    entries
        .iter()
        .find(|(path, _)| path == &wanted)
        .and_then(|(_, body)| *body)
}

fn top_string(value: &toml::Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn hugo_version_min(value: &toml::Value) -> Option<String> {
    let text = value
        .get("module")?
        .get("hugoVersion")?
        .get("min")?
        .as_str()?
        .trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn normalize_theme_id(name: &str) -> Option<String> {
    let mut out = String::new();
    let mut pending_hyphen = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            pending_hyphen = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.chars().count() > 80 {
        out = out.chars().take(80).collect();
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(path: &'a str, size: u64, body: Option<&'a str>) -> ThemeFile<'a> {
        ThemeFile { path, size, body }
    }

    #[test]
    fn rejects_unsafe_oversized_archives() {
        assert!(matches!(
            inspect_theme(&[file("../etc/passwd", 1, None)]),
            Err(ThemeInspectError::UnsafePath { .. })
        ));
        assert!(matches!(
            inspect_theme(&[file("/etc/passwd", 1, None)]),
            Err(ThemeInspectError::UnsafePath { .. })
        ));
        assert!(matches!(
            inspect_theme(&[file(
                "theme.toml",
                MAX_THEME_BYTES + 1,
                Some("name=\"X\"\n")
            )]),
            Err(ThemeInspectError::TooLarge { .. })
        ));
    }

    #[test]
    fn github_wrapper_prefers_theme_toml_over_example_site() {
        let theme = "name = \"PaperMod\"\nmin_version = \"0.146.0\"\nversion = \"8\"\n";
        let config = "name = \"Wrong\"\n\n[module.hugoVersion]\nmin = \"0.100.0\"\n";
        let info = inspect_theme(&[
            file("hugo-PaperMod-master/theme.toml", 10, Some(theme)),
            file("hugo-PaperMod-master/layouts/single.html", 4, None),
            file(
                "hugo-PaperMod-master/exampleSite/config.toml",
                8,
                Some(config),
            ),
            file(
                "__MACOSX/hugo-PaperMod-master/theme.toml",
                4,
                Some("name = \"Junk\"\n"),
            ),
        ])
        .unwrap();
        assert_eq!(info.root, "hugo-PaperMod-master");
        assert_eq!(info.name, "PaperMod");
        assert_eq!(info.id, "papermod");
        assert_eq!(info.min_version.as_deref(), Some("0.146.0"));
        assert_eq!(info.version.as_deref(), Some("8"));
    }

    #[test]
    fn module_hugo_version_when_theme_toml_has_no_min() {
        let config = "name = \"Blowfish\"\n\n[module.hugoVersion]\nmin = \"0.164.0\"\n";
        let info = inspect_theme(&[file("blowfish-main/config.toml", 20, Some(config))]).unwrap();
        assert_eq!(info.id, "blowfish");
        assert_eq!(info.min_version.as_deref(), Some("0.164.0"));
        assert_eq!(info.root, "blowfish-main");
    }
}
