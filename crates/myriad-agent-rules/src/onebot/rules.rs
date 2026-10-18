//! OneBot 私聊通道的纯规则：开关、能力位、打字状态、握手失败归类。无 I/O。

use crate::channel::{ChannelCapabilities, ConnectFailureKind, WorkerIntent};

/// 开关打开、`ws_url` 去空白后非空、且已配置 token → 运行；否则停止。
pub fn onebot_worker_intent(enabled: bool, ws_url: &str, has_token: bool) -> WorkerIntent {
    if enabled && !ws_url.trim().is_empty() && has_token {
        WorkerIntent::Run
    } else {
        WorkerIntent::Stop
    }
}

/// OneBot 私聊：文本与媒体入站，最终文本与图片出站。
/// 没有按钮段，澄清靠编号文本；只有撤回，没有编辑。
/// 打字状态走 `set_input_status`，不占能力位，见 [`onebot_worker_supports_typing`]。
pub fn onebot_private_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: true,
        inbound_callback: false,
        outbound_final_text: true,
        // SnowLuma sends markdown only as a flash-message card, not as a
        // segment of a normal message. Structured results are rendered as
        // readable text and sent as a text segment.
        outbound_markdown: false,
        outbound_image: true,
        outbound_edit: false,
        outbound_streaming_draft: false,
        interactive: true,
        frontend_action: false,
        performance: false,
        outfit: false,
    }
}

/// OneBot 有 `set_input_status`。既有通道的打字状态不在能力位里，这里是单一事实来源。
pub fn onebot_worker_supports_typing() -> bool {
    true
}

/// SnowLuma 在 WebSocket 升级阶段鉴权：token 不对回 HTTP 401，路径不对回 400，连接根本建不起来。
/// 4xx 都是「配置本身不对」，重试无用；5xx、缺失状态码（网络错误）可重试。
pub fn classify_onebot_handshake(status: Option<u16>) -> ConnectFailureKind {
    match status {
        Some(400..=499) => ConnectFailureKind::Permanent,
        _ => ConnectFailureKind::Transient,
    }
}

/// 群号白名单的分隔符：逗号、顿号、分号（全角半角都认）和空白。
fn group_allowlist_items(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(|c: char| matches!(c, ',' | '，' | '、' | ';' | '；') || c.is_whitespace())
        .filter(|item| !item.is_empty())
}

/// 把设置页填的群号名单规范化成升序、去重、逗号分隔。空名单是空串。
/// 有一项不是群号就整体拒绝，免得写错一个号却以为它在名单里。
pub fn normalize_onebot_group_allowlist(raw: &str) -> Result<String, String> {
    let mut ids = Vec::new();
    for item in group_allowlist_items(raw) {
        match item.parse::<u64>() {
            Ok(id) if id > 0 => ids.push(id),
            _ => return Err(format!("「{item}」不是群号")),
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids.iter().map(u64::to_string).collect::<Vec<_>>().join(","))
}

/// 这个群的消息能不能进来。名单为空：不限群。名单解不开：一个群都不进——
/// 宁可漏听，也不听到没被允许的群。
pub fn onebot_group_allowed(allowlist: &str, group_id: &str) -> bool {
    let Ok(allowlist) = normalize_onebot_group_allowlist(allowlist) else {
        return false;
    };
    allowlist.is_empty() || allowlist.split(',').any(|id| id == group_id.trim())
}
