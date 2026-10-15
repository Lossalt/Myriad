//! 笔记公开地址用的 slug。纯中文等 ASCII 化之后为空时返回 [`None`]，调用方改用 [`fallback_slug`]。

use unicode_normalization::UnicodeNormalization;

/// 博客挂在站点上的前缀。页面路径都在它下面。
pub const BLOG_PREFIX: &str = "/blog";

/// slug 最长字符数。按 Unicode 标量计，不是字节。
pub const SLUG_MAX_CHARS: usize = 80;

/// 占用表一直返回 true 时最多试到这个后缀，避免死循环。
const DEDUPE_LIMIT: u32 = 10_000;

/// 把标题收成 `[a-z0-9-]`。去首尾和重复的 `-`，截到 [`SLUG_MAX_CHARS`]。
/// 结果为空（纯中文、纯标点）时返回 `None`。
pub fn slugify_title(title: &str) -> Option<String> {
    let folded = fold_latin(title);
    let mut slug = String::new();
    let mut hyphen = false;
    for ch in folded.nfkd() {
        if is_combining_mark(ch) {
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            hyphen = false;
            continue;
        }
        // 其余字符（空格、标点、汉字）都当成一个分隔，不把词粘在一起。
        if !hyphen && !slug.is_empty() {
            slug.push('-');
            hyphen = true;
        }
        if slug.chars().count() > SLUG_MAX_CHARS {
            break;
        }
    }
    trim_hyphens(&mut slug);
    if slug.chars().count() > SLUG_MAX_CHARS {
        slug = slug.chars().take(SLUG_MAX_CHARS).collect();
        trim_hyphens(&mut slug);
    }
    if slug.is_empty() { None } else { Some(slug) }
}

/// 标题 ASCII 化失败时的地址：`n` 加笔记 id。
pub fn fallback_slug(item_id: i32) -> String {
    format!("n{item_id}")
}

/// `slugify_title` / [`fallback_slug`] 产出的形状，外加管理员手改后仍要满足的约束。
pub fn validate_slug(slug: &str) -> bool {
    let len = slug.chars().count();
    if len == 0 || len > SLUG_MAX_CHARS {
        return false;
    }
    if slug.starts_with('-') || slug.ends_with('-') || slug.contains("--") {
        return false;
    }
    slug.chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

/// `taken` 为真则在后面加 `-2`、`-3`。结果仍不超过 [`SLUG_MAX_CHARS`]。
pub fn dedupe_slug(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = base.trim();
    if !taken(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = append_suffix(base, n);
        if !taken(&candidate) || n == DEDUPE_LIMIT {
            return candidate;
        }
        n += 1;
    }
}

/// 笔记的站内路径，含首尾斜杠。
pub fn post_path(slug: &str) -> String {
    format!("/blog/posts/{slug}/")
}

/// [`post_path`] 的别名。笔记公开地址是 `/blog/posts/{slug}/`。
pub fn note_blog_path(slug: &str) -> String {
    post_path(slug)
}

fn append_suffix(base: &str, n: u32) -> String {
    let suffix = format!("-{n}");
    let room = SLUG_MAX_CHARS.saturating_sub(suffix.chars().count());
    let mut stem: String = base.chars().take(room).collect();
    trim_hyphens(&mut stem);
    if stem.is_empty() {
        return format!("n{n}");
    }
    format!("{stem}{suffix}")
}

fn trim_hyphens(slug: &mut String) {
    while slug.ends_with('-') {
        slug.pop();
    }
    while slug.starts_with('-') {
        slug.remove(0);
    }
}

/// NFKD 拆不开的几个拉丁字母。其余字母交给 NFKD（全角字母、带音符的拉丁字母）。
fn fold_latin(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for ch in title.chars() {
        match ch {
            'ß' | 'ẞ' => out.push_str("ss"),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            'ø' | 'Ø' => out.push('o'),
            'ł' | 'Ł' => out.push('l'),
            'đ' | 'Đ' => out.push('d'),
            'ħ' | 'Ħ' => out.push('h'),
            'ı' => out.push('i'),
            'ŋ' | 'Ŋ' => out.push_str("ng"),
            other => out.push(other),
        }
    }
    out
}

fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
    )
}
