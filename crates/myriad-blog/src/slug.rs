//! 笔记公开地址用的 slug。算法只在 `myriad-phantasi`，这里保留博客导出原来的形状。

pub use myriad_phantasi::{BLOG_PREFIX, fallback_slug, post_path};

/// 与 [`myriad_phantasi::SLUG_MAX_LEN`] 相同。按 Unicode 标量计，产出都是 ASCII。
pub const SLUG_MAX_CHARS: usize = myriad_phantasi::SLUG_MAX_LEN;

/// 标题 ASCII 化。纯中文等结果为空时返回 [`None`]，调用方改用 [`fallback_slug`]。
pub fn slugify_title(title: &str) -> Option<String> {
    let slug = myriad_phantasi::slugify_title(title);
    if slug.is_empty() { None } else { Some(slug) }
}

/// 管理员手改后仍要满足的形状。和 [`myriad_phantasi::is_note_slug`] 同一套。
pub fn validate_slug(slug: &str) -> bool {
    myriad_phantasi::is_note_slug(slug)
}

/// `taken` 为真则在后面加 `-2`、`-3`。
pub fn dedupe_slug(base: &str, taken: impl Fn(&str) -> bool) -> String {
    myriad_phantasi::dedupe_slug(base, taken)
}

/// [`post_path`] 的别名。
pub fn note_blog_path(slug: &str) -> String {
    post_path(slug)
}
