//! 主题包：`theme.toml` 识别、主题 id，以及内置 `myriad-default` 的文件表。

use std::fmt;

/// Hugo 0.146 起模板放在 `layouts/` 根上，不再用 `layouts/_default/`。
/// `single.html` 是普通页的回退，`list.html` 是 section 的回退。
/// 404 不套 baseof，所以那一份是完整文档。
pub fn default_theme_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "theme.toml",
            include_str!("../theme/myriad-default/theme.toml"),
        ),
        (
            "layouts/baseof.html",
            include_str!("../theme/myriad-default/layouts/baseof.html"),
        ),
        (
            "layouts/home.html",
            include_str!("../theme/myriad-default/layouts/home.html"),
        ),
        (
            "layouts/list.html",
            include_str!("../theme/myriad-default/layouts/list.html"),
        ),
        (
            "layouts/single.html",
            include_str!("../theme/myriad-default/layouts/single.html"),
        ),
        (
            "layouts/taxonomy.html",
            include_str!("../theme/myriad-default/layouts/taxonomy.html"),
        ),
        (
            "layouts/term.html",
            include_str!("../theme/myriad-default/layouts/term.html"),
        ),
        (
            "layouts/404.html",
            include_str!("../theme/myriad-default/layouts/404.html"),
        ),
        (
            "static/css/style.css",
            include_str!("../theme/myriad-default/static/css/style.css"),
        ),
    ]
}

/// 从 `theme.toml` 文本里读出的主题信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeMeta {
    pub name: String,
    pub min_version: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub tags: Vec<String>,
}

/// `theme.toml` 读不出来的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeManifestError {
    /// 不是合法 TOML。`message` 是解析器的短说明，不含文件正文。
    Invalid(String),
    /// 没有非空的 `name`。
    MissingName,
}

impl fmt::Display for ThemeManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid theme.toml: {message}"),
            Self::MissingName => write!(f, "theme.toml is missing a name"),
        }
    }
}

impl std::error::Error for ThemeManifestError {}

/// 解析主题清单。`min_version` 不是字符串时留空，不因此拒绝整份清单。
pub fn inspect_theme_manifest(text: &str) -> Result<ThemeMeta, ThemeManifestError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let table: toml::Table = toml::from_str(text)
        .map_err(|err: toml::de::Error| ThemeManifestError::Invalid(err.to_string()))?;
    let table = &table;
    let name = table
        .get("name")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or(ThemeManifestError::MissingName)?;
    Ok(ThemeMeta {
        name: name.to_string(),
        min_version: optional_string(table, "min_version"),
        description: optional_string(table, "description"),
        license: optional_string(table, "license"),
        homepage: optional_string(table, "homepage"),
        tags: string_array(table, "tags"),
    })
}

/// 主题目录名。只允许 ASCII 字母、数字、`-`、`_`、`.`，且不能是路径。
pub fn sanitize_theme_id(id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() || id.chars().count() > 100 {
        return None;
    }
    let mut chars = id.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphanumeric() {
        return None;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')) {
        return None;
    }
    if id.contains("..") {
        return None;
    }
    Some(id.to_string())
}

fn optional_string(table: &toml::map::Map<String, toml::Value>, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn string_array(table: &toml::map::Map<String, toml::Value>, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
