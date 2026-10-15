//! 主题安装与卸载。zip 在这里解，规则在 `myriad-blog`。

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use chrono::Utc;
use myriad_blog::{
    DEFAULT_THEME_ID, MAX_THEME_ARCHIVE_BYTES, MAX_THEME_ARCHIVE_ENTRIES, MAX_THEME_BYTES,
    MAX_THEME_FILES, ThemeFile, archive_entry_path, default_theme_files, github_archive_root,
    inspect_theme, inspect_theme_manifest, is_theme_archive_noise, safe_archive_path,
};
use serde::{Deserialize, Serialize};

use super::layout::BlogLayout;

const META_FILE: &str = ".myriad-theme.json";

#[derive(Debug)]
pub struct ThemeRejected {
    pub status: u16,
    pub message: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThemeRecord {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub min_version: Option<String>,
    pub builtin: bool,
    pub installed_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ThemeSidecar {
    id: String,
    name: String,
    version: Option<String>,
    min_version: Option<String>,
    builtin: bool,
    installed_at: String,
}

pub fn ensure_default_theme(layout: &BlogLayout) -> Result<(), ThemeRejected> {
    let dir = layout.theme_dir(DEFAULT_THEME_ID);
    if dir.join("theme.toml").is_file() {
        if !dir.join(META_FILE).is_file() {
            write_sidecar(
                &dir,
                &ThemeRecord {
                    id: DEFAULT_THEME_ID.to_string(),
                    name: "Myriad Default".to_string(),
                    version: None,
                    min_version: Some("0.146.0".to_string()),
                    builtin: true,
                    installed_at: Some(Utc::now().to_rfc3339()),
                },
            )?;
        }
        return Ok(());
    }
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|_| storage("Could not reset the default theme"))?;
    }
    fs::create_dir_all(&dir).map_err(|_| storage("Could not install the default theme"))?;
    for (rel, body) in default_theme_files() {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| storage("Could not install the default theme"))?;
        }
        fs::write(&path, body).map_err(|_| storage("Could not install the default theme"))?;
    }
    write_sidecar(
        &dir,
        &ThemeRecord {
            id: DEFAULT_THEME_ID.to_string(),
            name: "Myriad Default".to_string(),
            version: None,
            min_version: Some("0.146.0".to_string()),
            builtin: true,
            installed_at: Some(Utc::now().to_rfc3339()),
        },
    )
}

pub fn list_themes(layout: &BlogLayout) -> Vec<ThemeRecord> {
    let _ = ensure_default_theme(layout);
    let mut themes = Vec::new();
    let Ok(entries) = fs::read_dir(layout.themes_dir()) else {
        return themes;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str() else {
            continue;
        };
        if id.starts_with('.') || !entry.path().is_dir() {
            continue;
        }
        if let Some(record) = read_theme(layout, id) {
            themes.push(record);
        }
    }
    themes.sort_by(|left, right| left.id.cmp(&right.id));
    themes
}

pub fn read_theme(layout: &BlogLayout, id: &str) -> Option<ThemeRecord> {
    let dir = layout.theme_dir(id);
    if !dir.is_dir() {
        return None;
    }
    if let Some(record) = read_sidecar(&dir) {
        return Some(record);
    }
    let text = fs::read_to_string(dir.join("theme.toml")).ok()?;
    let meta = inspect_theme_manifest(&text).ok()?;
    Some(ThemeRecord {
        id: id.to_string(),
        name: meta.name,
        version: None,
        min_version: meta.min_version,
        builtin: id == DEFAULT_THEME_ID,
        installed_at: None,
    })
}

pub fn install_zip(layout: &BlogLayout, bytes: &[u8]) -> Result<ThemeRecord, ThemeRejected> {
    if bytes.len() as u64 > MAX_THEME_ARCHIVE_BYTES {
        return Err(ThemeRejected {
            status: 413,
            message: "Theme archive is too large",
        });
    }
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|_| ThemeRejected {
        status: 400,
        message: "Theme archive is not a zip file",
    })?;
    if archive.len() > MAX_THEME_ARCHIVE_ENTRIES {
        return Err(ThemeRejected {
            status: 400,
            message: "Theme archive has too many files",
        });
    }

    let mut names = Vec::new();
    let mut total = 0u64;
    let mut kept_files = 0usize;
    for index in 0..archive.len() {
        let file = archive.by_index_raw(index).map_err(|_| ThemeRejected {
            status: 400,
            message: "Theme archive could not be read",
        })?;
        let name = file.name().to_string();
        if !name.ends_with('/') && safe_archive_path(&name).is_none() {
            return Err(ThemeRejected {
                status: 400,
                message: "Theme archive contains an unsafe path",
            });
        }
        if is_theme_archive_noise(&name) {
            continue;
        }
        if file.is_symlink() {
            return Err(ThemeRejected {
                status: 400,
                message: "Theme archive contains a symlink",
            });
        }
        if !file.is_dir() {
            total = total.saturating_add(file.size());
            kept_files += 1;
            if kept_files > MAX_THEME_FILES {
                return Err(ThemeRejected {
                    status: 400,
                    message: "Theme archive has too many files",
                });
            }
        }
        names.push((index, name));
    }
    if total > MAX_THEME_BYTES {
        return Err(ThemeRejected {
            status: 413,
            message: "Theme archive is too large",
        });
    }

    let strip = github_archive_root(names.iter().map(|(_, name)| name.as_str()));
    let mut files = Vec::new();
    for (index, name) in &names {
        if name.ends_with('/') {
            continue;
        }
        let Some(rel) = archive_entry_path(name, strip.as_deref()) else {
            continue;
        };
        let rel_text = rel.to_string_lossy().replace('\\', "/");
        let file = archive.by_index(*index).map_err(|_| ThemeRejected {
            status: 400,
            message: "Theme archive could not be read",
        })?;
        let size = file.size();
        let is_manifest = rel_text.ends_with("theme.toml")
            || rel_text.ends_with("hugo.toml")
            || rel_text.ends_with("config.toml");
        let body = if is_manifest {
            let mut text = String::new();
            file.take(256 * 1024)
                .read_to_string(&mut text)
                .map_err(|_| ThemeRejected {
                    status: 400,
                    message: "Theme archive could not be read",
                })?;
            Some(text)
        } else {
            None
        };
        files.push((*index, rel_text, size, body));
    }
    let inspect_files: Vec<ThemeFile<'_>> = files
        .iter()
        .map(|(_, path, size, body)| ThemeFile {
            path,
            size: *size,
            body: body.as_deref(),
        })
        .collect();
    let info = inspect_theme(&inspect_files).map_err(|error| ThemeRejected {
        status: 400,
        message: match error {
            myriad_blog::ThemeInspectError::TooManyFiles { .. } => {
                "Theme archive has too many files"
            }
            myriad_blog::ThemeInspectError::TooLarge { .. } => "Theme archive is too large",
            myriad_blog::ThemeInspectError::UnsafePath { .. } => {
                "Theme archive contains an unsafe path"
            }
            myriad_blog::ThemeInspectError::NotATheme => "Archive is not a Hugo theme",
            myriad_blog::ThemeInspectError::InvalidId => "Theme name cannot be used as an id",
        },
    })?;
    if info.id == DEFAULT_THEME_ID {
        return Err(ThemeRejected {
            status: 400,
            message: "The built-in theme cannot be replaced from an archive",
        });
    }

    let staging = layout.themes_dir().join(format!(
        ".staging-{}-{}",
        info.id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    if let Err(error) = (|| -> Result<(), ThemeRejected> {
        fs::create_dir_all(&staging).map_err(|_| storage("Could not store the theme"))?;
        let mut written = 0u64;
        for (index, rel, _, _) in &files {
            let Some(stripped) = strip_theme_root(rel, &info.root) else {
                continue;
            };
            if stripped.is_empty() {
                continue;
            }
            let dest = staging.join(&stripped);
            if !dest.starts_with(&staging) {
                return Err(ThemeRejected {
                    status: 400,
                    message: "Theme archive contains an unsafe path",
                });
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|_| storage("Could not store the theme"))?;
            }
            let mut file = archive.by_index(*index).map_err(|_| ThemeRejected {
                status: 400,
                message: "Theme archive could not be read",
            })?;
            let mut output =
                fs::File::create(&dest).map_err(|_| storage("Could not store the theme"))?;
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut buf).map_err(|_| ThemeRejected {
                    status: 400,
                    message: "Theme archive could not be read",
                })?;
                if n == 0 {
                    break;
                }
                written = written.saturating_add(n as u64);
                if written > MAX_THEME_BYTES {
                    return Err(ThemeRejected {
                        status: 413,
                        message: "Theme archive is too large",
                    });
                }
                output
                    .write_all(&buf[..n])
                    .map_err(|_| storage("Could not store the theme"))?;
            }
        }
        let record = ThemeRecord {
            id: info.id.clone(),
            name: info.name.clone(),
            version: info.version.clone(),
            min_version: info.min_version.clone(),
            builtin: false,
            installed_at: Some(Utc::now().to_rfc3339()),
        };
        write_sidecar(&staging, &record)?;
        let dest = layout.theme_dir(&info.id);
        if dest.exists() {
            fs::remove_dir_all(&dest).map_err(|_| storage("Could not replace the theme"))?;
        }
        fs::rename(&staging, &dest).map_err(|_| storage("Could not store the theme"))?;
        Ok(())
    })() {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    read_theme(layout, &info.id).ok_or(ThemeRejected {
        status: 500,
        message: "Could not store the theme",
    })
}

pub fn uninstall_theme(layout: &BlogLayout, id: &str) -> Result<bool, ThemeRejected> {
    let Some(id) = myriad_blog::sanitize_theme_id(id) else {
        return Err(ThemeRejected {
            status: 404,
            message: "Theme not found",
        });
    };
    if id == DEFAULT_THEME_ID {
        return Err(ThemeRejected {
            status: 400,
            message: "The built-in theme cannot be removed",
        });
    }
    let Some(record) = read_theme(layout, &id) else {
        return Err(ThemeRejected {
            status: 404,
            message: "Theme not found",
        });
    };
    if record.builtin {
        return Err(ThemeRejected {
            status: 400,
            message: "The built-in theme cannot be removed",
        });
    }
    fs::remove_dir_all(layout.theme_dir(&id)).map_err(|_| storage("Could not remove the theme"))?;
    Ok(true)
}

pub fn copy_theme_into(layout: &BlogLayout, id: &str, dest: &Path) -> Result<(), &'static str> {
    let src = layout.theme_dir(id);
    if !src.join("theme.toml").is_file() && !src.join("layouts").is_dir() {
        return Err("Theme is not installed");
    }
    copy_tree(&src, dest)
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), &'static str> {
    fs::create_dir_all(dest).map_err(|_| "Could not prepare the theme")?;
    let entries = fs::read_dir(src).map_err(|_| "Could not read the theme")?;
    for entry in entries {
        let entry = entry.map_err(|_| "Could not read the theme")?;
        let file_type = entry.file_type().map_err(|_| "Could not read the theme")?;
        if file_type.is_symlink() {
            return Err("Theme contains a symlink");
        }
        let target = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).map_err(|_| "Could not copy the theme")?;
        }
    }
    Ok(())
}

fn strip_theme_root(path: &str, root: &str) -> Option<String> {
    if root.is_empty() {
        return Some(path.to_string());
    }
    if path == root {
        return Some(String::new());
    }
    path.strip_prefix(&format!("{root}/")).map(str::to_string)
}

fn write_sidecar(dir: &Path, record: &ThemeRecord) -> Result<(), ThemeRejected> {
    let sidecar = ThemeSidecar {
        id: record.id.clone(),
        name: record.name.clone(),
        version: record.version.clone(),
        min_version: record.min_version.clone(),
        builtin: record.builtin,
        installed_at: record
            .installed_at
            .clone()
            .unwrap_or_else(|| Utc::now().to_rfc3339()),
    };
    let body =
        serde_json::to_vec_pretty(&sidecar).map_err(|_| storage("Could not store the theme"))?;
    fs::write(dir.join(META_FILE), body).map_err(|_| storage("Could not store the theme"))
}

fn read_sidecar(dir: &Path) -> Option<ThemeRecord> {
    let body = fs::read(dir.join(META_FILE)).ok()?;
    let sidecar: ThemeSidecar = serde_json::from_slice(&body).ok()?;
    Some(ThemeRecord {
        id: sidecar.id,
        name: sidecar.name,
        version: sidecar.version,
        min_version: sidecar.min_version,
        builtin: sidecar.builtin,
        installed_at: Some(sidecar.installed_at),
    })
}

fn storage(message: &'static str) -> ThemeRejected {
    ThemeRejected {
        status: 500,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn layout_at(root: &Path) -> BlogLayout {
        BlogLayout {
            root: root.to_path_buf(),
            hugo_cache: root.join("cache"),
        }
    }

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, body) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
        cursor.into_inner()
    }

    #[test]
    fn zip_slip_and_symlinks_are_rejected() {
        let root = std::env::temp_dir().join(format!(
            "myriad-blog-theme-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let layout = layout_at(&root);
        let slip = zip_bytes(&[("../evil.toml", b"name = \"Evil\"\n")]);
        let error = install_zip(&layout, &slip).unwrap_err();
        assert_eq!(error.message, "Theme archive contains an unsafe path");

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("theme.toml", options).unwrap();
        writer.write_all(b"name = \"Linked\"\n").unwrap();
        writer.start_file("layouts/home.html", options).unwrap();
        writer.write_all(b"home").unwrap();
        writer
            .add_symlink("static/escape", "/etc/passwd", options)
            .unwrap();
        writer.finish().unwrap();
        let link = cursor.into_inner();
        let error = install_zip(&layout, &link).unwrap_err();
        assert_eq!(error.message, "Theme archive contains a symlink");
        assert!(
            !root.join("themes").exists()
                || fs::read_dir(root.join("themes")).unwrap().next().is_none()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_theme_zip_installs_under_its_id_and_the_default_cannot_be_removed() {
        let root = std::env::temp_dir().join(format!(
            "myriad-blog-theme-ok-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let layout = layout_at(&root);
        ensure_default_theme(&layout).unwrap();
        let error = uninstall_theme(&layout, DEFAULT_THEME_ID).unwrap_err();
        assert_eq!(error.message, "The built-in theme cannot be removed");

        let packed = zip_bytes(&[
            (
                "PaperMod-master/theme.toml",
                b"name = \"PaperMod\"\nmin_version = \"0.100.0\"\n",
            ),
            ("PaperMod-master/layouts/home.html", b"<h1>home</h1>"),
        ]);
        let record = install_zip(&layout, &packed).unwrap();
        assert_eq!(record.id, "papermod");
        assert_eq!(record.name, "PaperMod");
        assert!(!record.builtin);
        assert!(
            layout
                .theme_dir("papermod")
                .join("layouts/home.html")
                .is_file()
        );
        assert!(
            !layout
                .theme_dir("papermod")
                .join("PaperMod-master")
                .exists()
        );
        uninstall_theme(&layout, "papermod").unwrap();
        assert!(!layout.theme_dir("papermod").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn example_sites_and_repository_files_do_not_count_against_the_limits() {
        let root = std::env::temp_dir().join(format!(
            "myriad-blog-theme-noise-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let layout = layout_at(&root);
        let example: Vec<String> = (0..=MAX_THEME_FILES)
            .map(|index| format!("blowfish-main/exampleSite/content/p{index}.md"))
            .collect();
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("blowfish-main/theme.toml", b"name = \"Blowfish\"\n"),
            ("blowfish-main/layouts/home.html", b"home"),
            ("blowfish-main/.git/HEAD", b"ref: refs/heads/main\n"),
            ("__MACOSX/blowfish-main/._theme.toml", b"junk"),
        ];
        entries.extend(example.iter().map(|name| (name.as_str(), b"x".as_slice())));
        let record = install_zip(&layout, &zip_bytes(&entries)).unwrap();
        assert_eq!(record.id, "blowfish");
        let dir = layout.theme_dir("blowfish");
        assert!(dir.join("layouts/home.html").is_file());
        assert!(!dir.join("exampleSite").exists());
        assert!(!dir.join(".git").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
