//! 摘要：去掉公式 span 和脚注，压缩空白，按字符截断。

use crate::rewrite::{html_text, strip_for_summary};

/// 摘要最长字符数。按 Unicode 标量计。
pub const SUMMARY_MAX_CHARS: usize = 160;

/// `source` 可以是纯文本，也可以是 HTML。公式 span 整段去掉，不把 TeX 留在摘要里。
pub fn summarize(source: &str) -> String {
    let stripped = strip_for_summary(source);
    let text = decode_basic_entities(&html_text(&stripped));
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, SUMMARY_MAX_CHARS)
}

/// lol_html 的文本块是源码里的样子，实体还在。只解笔记消毒器会写出的那几个。
fn decode_basic_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}
