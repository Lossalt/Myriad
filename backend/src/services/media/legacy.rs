//! Legacy on-disk locations. Mapping is prefix-based; request URLs never join the disk.

use std::path::{Path, PathBuf};

use crate::services::data_paths::DataPaths;

use super::urls::registered_local_path;
use super::validate::extension_for_mime;

#[derive(Clone, Debug)]
pub struct LegacyPaths {
    pub federation_root: PathBuf,
    pub cache_images: PathBuf,
}

impl LegacyPaths {
    pub fn from_data_paths(paths: &DataPaths) -> Self {
        Self {
            federation_root: paths.root.join("federation_media"),
            cache_images: paths.cache_images.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyOwner {
    User(i32),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyKind {
    Federation,
    ImageCache,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyClass {
    CopyReady,
    Missing,
    Shared,
    UnknownOwner,
    OrphanCandidate,
    ForeignOrigin,
}

pub fn classify_uncited_cache_file() -> LegacyClass {
    LegacyClass::OrphanCandidate
}

pub fn ext_from_legacy(mime: &str, name_or_path: &str) -> Option<&'static str> {
    if let Some(ext) = extension_for_mime(mime) {
        return Some(ext);
    }
    let ext = Path::new(name_or_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        "mp4" => Some("mp4"),
        "webm" => Some("webm"),
        "mov" => Some("mov"),
        _ => None,
    }
}

pub fn owner_from_local_path(local_path: &str) -> LegacyOwner {
    let Some(path) = registered_local_path(local_path) else {
        return LegacyOwner::Unknown;
    };
    let Some(rest) = path.strip_prefix("/media/federation/") else {
        return LegacyOwner::Unknown;
    };
    let Some((user, _)) = rest.split_once('/') else {
        return LegacyOwner::Unknown;
    };
    match user.parse::<i32>() {
        Ok(id) if id > 0 => LegacyOwner::User(id),
        _ => LegacyOwner::Unknown,
    }
}

pub fn legacy_kind(local_path: &str) -> Option<LegacyKind> {
    let path = registered_local_path(local_path)?;
    if path.starts_with("/media/federation/") {
        Some(LegacyKind::Federation)
    } else if path.starts_with("/api/phantasi/image-cache/")
        || path.starts_with("/api/brew/image-cache/")
    {
        Some(LegacyKind::ImageCache)
    } else {
        None
    }
}

/// Historical brew cache URLs occupy the same files as phantasi cache URLs.
pub fn brew_alias_of(local_path: &str) -> Option<String> {
    let path = registered_local_path(local_path)?;
    path.strip_prefix("/api/phantasi/image-cache/")
        .map(|rest| format!("/api/brew/image-cache/{rest}"))
}

/// Both URL generations address the same validated cache file.
pub fn cache_equivalent_path(local_path: &str) -> Option<String> {
    let path = registered_local_path(local_path)?;
    if let Some(rest) = path.strip_prefix("/api/brew/image-cache/") {
        Some(format!("/api/phantasi/image-cache/{rest}"))
    } else {
        brew_alias_of(&path)
    }
}

pub fn legacy_disk_path(paths: &LegacyPaths, local_path: &str) -> Option<PathBuf> {
    let path = registered_local_path(local_path)?;
    if let Some(rest) = path.strip_prefix("/media/federation/") {
        return federation_file(&paths.federation_root, rest);
    }
    if let Some(rest) = path
        .strip_prefix("/api/phantasi/image-cache/")
        .or_else(|| path.strip_prefix("/api/brew/image-cache/"))
    {
        return image_cache_file(&paths.cache_images, rest, false);
    }
    None
}

/// Like [`legacy_disk_path`], for serving only. The image cache still writes
/// AVIF and SVG, which the old static route served; they are not importable
/// assets, so migration keeps using the strict form above.
pub fn legacy_serve_path(paths: &LegacyPaths, local_path: &str) -> Option<PathBuf> {
    let path = registered_local_path(local_path)?;
    if let Some(rest) = path
        .strip_prefix("/api/phantasi/image-cache/")
        .or_else(|| path.strip_prefix("/api/brew/image-cache/"))
    {
        return image_cache_file(&paths.cache_images, rest, true);
    }
    legacy_disk_path(paths, &path)
}

fn federation_file(root: &Path, rest: &str) -> Option<PathBuf> {
    let (user, file) = rest.split_once('/')?;
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return None;
    }
    if !user.chars().all(|c| c.is_ascii_digit()) || user.is_empty() {
        return None;
    }
    if file.is_empty()
        || file.starts_with('.')
        || !file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(root.join(user).join(file))
}

fn image_cache_file(root: &Path, rest: &str, display_only: bool) -> Option<PathBuf> {
    let (subdir, file) = rest.split_once('/')?;
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return None;
    }
    let (stem, ext) = file.rsplit_once('.')?;
    if subdir.len() != 2 || !subdir.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if stem.len() != 64 || !stem.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let ext = ext.to_ascii_lowercase();
    let importable = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp");
    if !importable && !(display_only && matches!(ext.as_str(), "avif" | "svg")) {
        return None;
    }
    let stem = stem.to_ascii_lowercase();
    let subdir = subdir.to_ascii_lowercase();
    if !stem.starts_with(&subdir) {
        return None;
    }
    Some(root.join(subdir).join(format!("{stem}.{ext}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> LegacyPaths {
        LegacyPaths {
            federation_root: PathBuf::from("/data/federation_media"),
            cache_images: PathBuf::from("/cache/images"),
        }
    }

    #[test]
    fn cached_avif_and_svg_are_servable_but_not_importable() {
        let stem = format!("ab{}", "0".repeat(62));
        for ext in ["avif", "svg"] {
            let url = format!("/api/phantasi/image-cache/ab/{stem}.{ext}");
            assert!(legacy_disk_path(&paths(), &url).is_none());
            assert_eq!(
                legacy_serve_path(&paths(), &url),
                Some(PathBuf::from(format!("/cache/images/ab/{stem}.{ext}")))
            );
        }
        let html = format!("/api/phantasi/image-cache/ab/{stem}.html");
        assert!(legacy_serve_path(&paths(), &html).is_none());
    }

    #[test]
    fn federation_paths_stay_inside_user_dirs() {
        let disk = legacy_disk_path(&paths(), "/media/federation/7/a-b_c.png").unwrap();
        assert_eq!(disk, PathBuf::from("/data/federation_media/7/a-b_c.png"));
        assert_eq!(
            owner_from_local_path("/media/federation/7/a.png"),
            LegacyOwner::User(7)
        );
        assert_eq!(
            owner_from_local_path("/media/federation/0/a.png"),
            LegacyOwner::Unknown
        );
        assert!(legacy_disk_path(&paths(), "/media/federation/7/../secret").is_none());
        assert!(legacy_disk_path(&paths(), "/media/federation/7/%2e%2e").is_none());
        assert!(legacy_disk_path(&paths(), "/media/federation/7/a/b.png").is_none());
    }

    #[test]
    fn image_cache_paths_require_hash_layout() {
        let hash = "ab".to_string() + &"c".repeat(62);
        let url = format!("/api/phantasi/image-cache/ab/{hash}.png");
        let disk = legacy_disk_path(&paths(), &url).unwrap();
        assert_eq!(disk, PathBuf::from(format!("/cache/images/ab/{hash}.png")));
        assert_eq!(
            brew_alias_of(&url).as_deref(),
            Some(format!("/api/brew/image-cache/ab/{hash}.png").as_str())
        );
        assert_eq!(owner_from_local_path(&url), LegacyOwner::Unknown);
        assert!(legacy_disk_path(&paths(), "/api/phantasi/image-cache/ab/../x.png").is_none());
        assert!(
            legacy_disk_path(
                &paths(),
                &format!("/api/phantasi/image-cache/cd/{hash}.png")
            )
            .is_none()
        );
        let brew = format!("/api/brew/image-cache/ab/{hash}.webp");
        assert!(legacy_disk_path(&paths(), &brew).is_some());
    }

    #[test]
    fn uncited_cache_files_are_candidates_not_imports() {
        assert_eq!(classify_uncited_cache_file(), LegacyClass::OrphanCandidate);
    }
}
