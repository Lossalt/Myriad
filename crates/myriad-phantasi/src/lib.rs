//! Pure Phantasi domain rules.
//!
//! Path constants.
//! No database, HTTP, or Axum.

pub mod feed_topic_cards;

pub use feed_topic_cards::{
    FEED_TOPIC_CARDS_KEY, feed_topic_cards_from_value, sanitize_feed_topic_cards,
};

/// Public blog root. Notes are addressed under this prefix.
pub const BLOG_PREFIX: &str = "/blog";

/// User-visible public prefix for notes. Internal module key stays `phantasi`.
pub const SPA_PREFIX: &str = BLOG_PREFIX;

/// ActivityPub object prefix. Changing article IDs under this path breaks federation.
pub const ARTICLE_OBJECT_PREFIX: &str = "/phantasi";

/// Public notes RSS. The old `/journal/notes.xml` shell is gone.
pub const NOTES_RSS_PATH: &str = "/api/phantasi/notes.xml";

/// Platform config key for the notes RSS switch.
pub const NOTES_RSS_PREFERENCES_KEY: &str = "phantasi_notes_rss";

/// Longest note slug, including a collision suffix.
pub const SLUG_MAX_LEN: usize = 80;

/// Public note path. The address is the slug, with a trailing slash.
pub fn post_path(slug: &str) -> String {
    format!("{BLOG_PREFIX}/posts/{slug}/")
}

/// Public note path. Same as [`post_path`].
pub fn item_path(slug: &str) -> String {
    post_path(slug)
}

/// ActivityPub object ID. Changing this breaks federation.
pub fn article_federation_path(item_id: i32) -> String {
    format!("{ARTICLE_OBJECT_PREFIX}/articles/{item_id}")
}

/// Slug used when a title has no ASCII letters or digits.
pub fn fallback_slug(item_id: i32) -> String {
    format!("n{item_id}")
}

/// Lowercase ASCII slug: `[a-z0-9-]`, no leading or trailing `-`, at most 80 characters.
///
/// Latin letters are folded first (`Café` → `cafe`, `Straße` → `strasse`, fullwidth
/// letters via NFKD). Everything else collapses into a single `-`. An empty result
/// means the title had no ASCII letters or digits.
pub fn slugify_title(title: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

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
        if !hyphen && !slug.is_empty() {
            slug.push('-');
            hyphen = true;
        }
        if slug.chars().count() > SLUG_MAX_LEN {
            break;
        }
    }
    trim_slug_hyphens(&mut slug);
    if slug.chars().count() > SLUG_MAX_LEN {
        slug = slug.chars().take(SLUG_MAX_LEN).collect();
        trim_slug_hyphens(&mut slug);
    }
    slug
}

/// A slug an administrator may store: the same shape [`slugify_title`] produces.
pub fn is_note_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.len() > SLUG_MAX_LEN {
        return false;
    }
    let bytes = slug.as_bytes();
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return false;
    }
    let mut prev_dash = false;
    for byte in bytes {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            prev_dash = false;
        } else if *byte == b'-' && !prev_dash {
            prev_dash = true;
        } else {
            return false;
        }
    }
    true
}

/// Choose a slug for `item_id`. An empty ASCII title becomes `n{item_id}`.
/// A value `taken` already holds gets `-2`, then `-3`, and so on.
pub fn allocate_slug(title: &str, item_id: i32, taken: impl Fn(&str) -> bool) -> String {
    let slug = slugify_title(title);
    let base = if slug.is_empty() {
        fallback_slug(item_id)
    } else {
        slug
    };
    dedupe_slug(&base, taken)
}

/// Append `-2`, `-3`, … while `taken` is true. The result stays within [`SLUG_MAX_LEN`].
///
/// Stops at suffix `10000` even if that candidate is still taken, so a broken
/// `taken` callback cannot loop forever.
pub fn dedupe_slug(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = base.trim();
    if !taken(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = slug_with_suffix(base, n);
        if !taken(&candidate) || n == 10_000 {
            return candidate;
        }
        n += 1;
    }
}

fn slug_with_suffix(stem: &str, n: u32) -> String {
    let suffix = format!("-{n}");
    let room = SLUG_MAX_LEN.saturating_sub(suffix.chars().count());
    let mut stem: String = stem.chars().take(room).collect();
    trim_slug_hyphens(&mut stem);
    if stem.is_empty() {
        return format!("n{n}");
    }
    format!("{stem}{suffix}")
}

fn trim_slug_hyphens(slug: &mut String) {
    while slug.ends_with('-') {
        slug.pop();
    }
    while slug.starts_with('-') {
        slug.remove(0);
    }
}

/// Latin letters NFKD does not split into ASCII. The rest is left for NFKD.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_do_not_contain_brew() {
        assert!(!NOTES_RSS_PATH.contains("brew"));
        assert_eq!(item_path("hello"), "/blog/posts/hello/");
        assert_eq!(post_path("hello"), "/blog/posts/hello/");
        assert_eq!(article_federation_path(12), "/phantasi/articles/12");
        assert_eq!(SPA_PREFIX, "/blog");
        assert_eq!(BLOG_PREFIX, "/blog");
        assert_eq!(NOTES_RSS_PATH, "/api/phantasi/notes.xml");
        assert_eq!(ARTICLE_OBJECT_PREFIX, "/phantasi");
        assert_eq!(FEED_TOPIC_CARDS_KEY, "phantasi_feed_topic_cards");
        assert!(!FEED_TOPIC_CARDS_KEY.contains("brew"));
    }

    #[test]
    fn slugify_keeps_ascii_words_and_drops_the_rest() {
        assert_eq!(slugify_title("Hello, World!"), "hello-world");
        assert_eq!(slugify_title("  A--B  "), "a-b");
        assert_eq!(slugify_title("你好"), "");
        assert_eq!(slugify_title("Café Notes"), "cafe-notes");
        assert_eq!(slugify_title("  Café Latte "), "cafe-latte");
        assert_eq!(slugify_title("Straße"), "strasse");
        assert_eq!(slugify_title("Ｈｅｌｌｏ"), "hello");
        assert_eq!(slugify_title("A世界B"), "a-b");
        assert_eq!(slugify_title("---"), "");
        assert_eq!(slugify_title(&"a".repeat(100)).len(), SLUG_MAX_LEN);
        assert!(is_note_slug("hello-world"));
        assert!(is_note_slug("n12"));
        assert!(!is_note_slug(""));
        assert!(!is_note_slug("-hello"));
        assert!(!is_note_slug("hello-"));
        assert!(!is_note_slug("hello--world"));
        assert!(!is_note_slug("Hello"));
    }

    #[test]
    fn allocate_uses_fallback_then_numeric_suffixes() {
        assert_eq!(fallback_slug(12), "n12");
        assert_eq!(allocate_slug("Hello", 12, |_| false), "hello");
        assert_eq!(allocate_slug("你好", 12, |_| false), "n12");
        assert_eq!(
            allocate_slug("Hello", 12, |slug| slug == "hello"),
            "hello-2"
        );
        assert_eq!(
            allocate_slug("Hello", 12, |slug| slug == "hello" || slug == "hello-2"),
            "hello-3"
        );
        let long = "a".repeat(80);
        let collided = allocate_slug(&long, 4, |slug| slug == long);
        assert!(collided.ends_with("-2"));
        assert!(collided.len() <= SLUG_MAX_LEN);
        assert!(is_note_slug(&collided));
        assert_ne!(collided, long);
    }
}
