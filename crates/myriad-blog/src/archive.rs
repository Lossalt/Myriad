//! zip 条目路径。拒绝绝对路径、`..`、盘符和用反斜杠做的穿越。
//!
//! GitHub `archive/refs/heads/<branch>.zip` 会在外面套一层 `仓库名-引用/`。
//! 所有安全条目都落在同一个顶层目录里时，[`github_archive_root`] 返回这个目录名，
//! 调用方再用 [`archive_entry_path`] 把它剥掉。已经是主题根（`theme.toml` 与 `layouts/` 并列）
//! 时没有共同的唯一目录，返回 `None`。

use std::path::{Path, PathBuf};

/// 校验一条 zip 路径。通过时返回只用 `/` 拼出来的相对路径。
pub fn safe_archive_path(entry: &str) -> Option<PathBuf> {
    if entry.is_empty() || entry.chars().any(char::is_control) {
        return None;
    }
    if entry.starts_with('/') || entry.starts_with('\\') {
        return None;
    }
    let bytes = entry.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }
    let normalized = entry.replace('\\', "/");
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return None;
    }
    Some(PathBuf::from(parts.join("/")))
}

/// 全部条目共享的顶层目录。有任意一条路径不合法，或者顶层不唯一，就返回 `None`。
pub fn github_archive_root<'a>(entries: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut root: Option<String> = None;
    let mut nested = false;
    let mut saw = false;
    for entry in entries {
        let path = safe_archive_path(entry)?;
        let text = path_text(&path);
        let mut comps = text.split('/');
        let first = comps.next()?;
        saw = true;
        if comps.next().is_some() {
            nested = true;
        }
        match &root {
            None => root = Some(first.to_string()),
            Some(existing) if existing == first => {}
            Some(_) => return None,
        }
    }
    if saw && nested { root } else { None }
}

/// `strip_root` 为 [`github_archive_root`] 的结果时，去掉这一层。
/// 条目就是这个目录本身时返回 `None`（目录壳，不是文件）。
/// 给了前缀但条目不在它下面，也返回 `None`。
pub fn archive_entry_path(entry: &str, strip_root: Option<&str>) -> Option<PathBuf> {
    let path = safe_archive_path(entry)?;
    let Some(root) = strip_root else {
        return Some(path);
    };
    if root.is_empty() || root.contains('/') || root.contains('\\') || root == "." || root == ".." {
        return None;
    }
    let text = path_text(&path);
    if text == root {
        return None;
    }
    let rest = text.strip_prefix(&format!("{root}/"))?;
    safe_archive_path(rest)
}

/// 整包校验。任一路径不合法就整包拒绝。GitHub 顶层目录会被剥掉。
pub fn safe_archive_entries<'a>(
    entries: impl IntoIterator<Item = &'a str>,
) -> Option<Vec<PathBuf>> {
    let list: Vec<&str> = entries.into_iter().collect();
    let root = github_archive_root(list.iter().copied());
    let mut out = Vec::new();
    for entry in list {
        match archive_entry_path(entry, root.as_deref()) {
            Some(path) => out.push(path),
            None if root
                .as_deref()
                .is_some_and(|root| is_root_entry(entry, root)) => {}
            None => return None,
        }
    }
    Some(out)
}

fn is_root_entry(entry: &str, root: &str) -> bool {
    safe_archive_path(entry).is_some_and(|path| path_text(&path) == root)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
