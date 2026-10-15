//! 封面：和正文第一张图相同就不写；按主题把地址填进对应的 front matter 键。

use serde_json::{Map, Value};

use crate::rewrite::first_content_image;
use crate::urlutil::absolutize_root_url;

/// 已知主题（id 里包含这一段）只写自己的封面键。点号表示嵌套，`cover.image` 写成对象。
const THEME_COVER_KEYS: &[(&str, &[&str])] = &[
    ("papermod", &["cover.image"]),
    ("blowfish", &["featureimage"]),
    ("ananke", &["featured_image"]),
    ("fixit", &["featured_image"]),
    ("stack", &["image"]),
];

/// 认不出的主题把这些键都写上。`images` 另外总是写成数组。
const ALL_COVER_KEYS: &[&str] = &["cover.image", "image", "featured_image", "featureimage"];

/// 封面和正文第一张图是同一张时返回 true。两边都会先按 `origin` 收成可比较的路径。
pub fn covers_same_asset(origin: &str, cover: &str, content_html: &str) -> bool {
    let Some(first) = first_content_image(content_html) else {
        return false;
    };
    normalize_asset(origin, cover) == normalize_asset(origin, &first)
}

/// 已知主题只写自己的封面键，再加通用 `images`。认不出的主题把这些键都写上。
pub fn cover_front_matter(theme_id: &str, image: &str) -> Map<String, Value> {
    let id = theme_id.to_ascii_lowercase();
    let keys = THEME_COVER_KEYS
        .iter()
        .find(|(needle, _)| id.contains(needle))
        .map(|(_, keys)| *keys)
        .unwrap_or(ALL_COVER_KEYS);
    let mut map = Map::new();
    for key in keys {
        insert_cover_key(&mut map, key, image);
    }
    map.insert(
        "images".to_string(),
        Value::Array(vec![Value::String(image.to_string())]),
    );
    map
}

fn insert_cover_key(map: &mut Map<String, Value>, key: &str, image: &str) {
    if let Some((parent, child)) = key.split_once('.') {
        let object = map
            .entry(parent.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(object) = object.as_object_mut() {
            object.insert(child.to_string(), Value::String(image.to_string()));
        }
    } else {
        map.insert(key.to_string(), Value::String(image.to_string()));
    }
}

pub(crate) fn absolutize_cover(origin: &str, cover: &str) -> String {
    absolutize_root_url(cover, origin)
}

fn normalize_asset(origin: &str, url: &str) -> String {
    let url = url.trim();
    let origin = origin.trim().trim_end_matches('/');
    if !origin.is_empty()
        && let Some(rest) = url.strip_prefix(origin)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return rest.to_string();
    }
    url.to_string()
}
