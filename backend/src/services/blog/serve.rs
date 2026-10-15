//! `/blog/*` 的路径解析。拒绝跑出当前输出目录，目录补上结尾斜杠。

use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub enum BlogTarget {
    Redirect(String),
    File(PathBuf),
    Missing,
}

/// `url_path` 是请求路径，例如 `/blog` 或 `/blog/posts/hello/`。
pub fn resolve_blog_target(current: &Path, url_path: &str) -> BlogTarget {
    let Some((rest, slash)) = blog_suffix(url_path) else {
        return BlogTarget::Missing;
    };
    if rest
        .split('/')
        .any(|part| part == ".." || part == "." || part.contains('\0'))
    {
        return BlogTarget::Missing;
    }
    if rest.contains('\\') {
        return BlogTarget::Missing;
    }
    let rel = rest.trim_matches('/');
    if !path_stays_inside(rel) {
        return BlogTarget::Missing;
    }
    let candidate = if rel.is_empty() {
        current.to_path_buf()
    } else {
        current.join(rel)
    };
    if is_dir_like(current, &candidate) {
        if !slash {
            let location = if rel.is_empty() {
                "/blog/".to_string()
            } else {
                format!("/blog/{rel}/")
            };
            return BlogTarget::Redirect(location);
        }
        let index = candidate.join("index.html");
        return if index.is_file() && stays_under(current, &index) {
            BlogTarget::File(index)
        } else {
            BlogTarget::Missing
        };
    }
    if candidate.is_file() && stays_under(current, &candidate) {
        BlogTarget::File(candidate)
    } else {
        BlogTarget::Missing
    }
}

pub fn theme_not_found_page(current: &Path) -> Option<PathBuf> {
    let page = current.join("404.html");
    if page.is_file() && stays_under(current, &page) {
        Some(page)
    } else {
        None
    }
}

/// `(相对路径, 请求是否已经以 / 结尾)`。`/blog` 本身不算已经带斜杠。
fn blog_suffix(url_path: &str) -> Option<(&str, bool)> {
    let path = url_path.split(['?', '#']).next().unwrap_or(url_path);
    if path == "/blog" {
        return Some(("", false));
    }
    let rest = path.strip_prefix("/blog/")?;
    Some((rest, rest.is_empty() || rest.ends_with('/')))
}

fn path_stays_inside(rel: &str) -> bool {
    if rel.is_empty() {
        return true;
    }
    Path::new(rel)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn is_dir_like(current: &Path, candidate: &Path) -> bool {
    if candidate.is_dir() {
        return true;
    }
    !candidate.exists()
        && candidate.join("index.html").is_file()
        && stays_under(current, &candidate.join("index.html"))
}

fn stays_under(root: &Path, path: &Path) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else {
        return false;
    };
    let Ok(path) = std::fs::canonicalize(path) else {
        return false;
    };
    path.starts_with(root)
}

pub fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "xml" => "application/xml; charset=utf-8",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "myriad-blog-serve-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(root.join("posts/hello")).unwrap();
        std::fs::write(root.join("index.html"), "home").unwrap();
        std::fs::write(root.join("posts/hello/index.html"), "post").unwrap();
        std::fs::write(root.join("404.html"), "missing").unwrap();
        std::fs::write(root.join("style.css"), "css").unwrap();
        root
    }

    #[test]
    fn directories_redirect_and_dotdot_is_rejected() {
        let root = tree();
        match resolve_blog_target(&root, "/blog") {
            BlogTarget::Redirect(location) => assert_eq!(location, "/blog/"),
            other => panic!("{other:?}"),
        }
        match resolve_blog_target(&root, "/blog/") {
            BlogTarget::File(path) => assert_eq!(path, root.join("index.html")),
            other => panic!("{other:?}"),
        }
        match resolve_blog_target(&root, "/blog/posts/hello") {
            BlogTarget::Redirect(location) => assert_eq!(location, "/blog/posts/hello/"),
            other => panic!("{other:?}"),
        }
        match resolve_blog_target(&root, "/blog/posts/hello/") {
            BlogTarget::File(path) => assert!(path.ends_with("posts/hello/index.html")),
            other => panic!("{other:?}"),
        }
        match resolve_blog_target(&root, "/blog/style.css") {
            BlogTarget::File(path) => assert!(path.ends_with("style.css")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            resolve_blog_target(&root, "/blog/../secret"),
            BlogTarget::Missing
        ));
        assert!(matches!(
            resolve_blog_target(&root, "/blog/posts/../../etc/passwd"),
            BlogTarget::Missing
        ));
        assert!(matches!(
            resolve_blog_target(&root, "/blogger"),
            BlogTarget::Missing
        ));
        assert!(theme_not_found_page(&root).is_some());
        std::fs::remove_dir_all(root).unwrap();
    }
}
