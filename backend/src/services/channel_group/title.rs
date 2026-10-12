//! What a group calls itself, as its platform says: so what she brings from
//! one group to another says where it came from. Asked once a day per
//! group, and not again for a while when the platform does not say.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::services::channel_platform::ChannelPlatform;

/// A name is asked again after this long: groups get renamed.
const KEPT_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// No name to be had is asked again after this long.
const MISSING_FOR: Duration = Duration::from_secs(10 * 60);
/// Group names are set by whoever runs the group: bounded.
const MAX_CHARS: usize = 40;

static KNOWN: LazyLock<Mutex<HashMap<String, (Option<String>, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// What the group `venue` (`<platform>:<chat id>`, as sessions keep it)
/// calls itself, if its platform says.
pub async fn title(venue: &str) -> Option<String> {
    let venue = venue.strip_prefix("group:").unwrap_or(venue);
    if let Some((name, at)) = KNOWN
        .lock()
        .ok()
        .and_then(|known| known.get(venue).cloned())
    {
        let lasts = if name.is_some() {
            KEPT_FOR
        } else {
            MISSING_FOR
        };
        if at.elapsed() < lasts {
            return name;
        }
    }
    let name = asked(venue).await.and_then(|name| clean(&name));
    if let Ok(mut known) = KNOWN.lock() {
        known.insert(venue.to_string(), (name.clone(), Instant::now()));
    }
    name
}

async fn asked(venue: &str) -> Option<String> {
    let (platform, chat) = venue.split_once(':')?;
    if platform == ChannelPlatform::OneBot.slug() {
        let group_id: i64 = chat.parse().ok()?;
        let info = crate::services::onebot_send::call_action(serde_json::json!({
            "action": "get_group_info",
            "params": { "group_id": group_id },
        }))
        .await
        .ok()?;
        return info.get("group_name")?.as_str().map(str::to_string);
    }
    if platform == ChannelPlatform::Telegram.slug() {
        let token = crate::GLOBAL_DYNAMIC_CONFIG
            .read()
            .await
            .telegram_bot_token
            .clone()?;
        return crate::services::telegram_bot::chat_title(&token, chat).await;
    }
    if platform == ChannelPlatform::Discord.slug() {
        let token = crate::GLOBAL_DYNAMIC_CONFIG
            .read()
            .await
            .discord_bot_token
            .clone()?;
        return crate::services::discord_bot::channel_name(&token, chat)
            .await
            .map(|name| format!("#{name}"));
    }
    None
}

fn clean(name: &str) -> Option<String> {
    let name: String = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_CHARS)
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_kept_short_and_on_one_line() {
        assert_eq!(clean("  摸鱼\n小队  "), Some("摸鱼 小队".into()));
        assert_eq!(clean(" \n "), None);
        assert_eq!(clean(&"长".repeat(80)).unwrap().chars().count(), MAX_CHARS);
    }
}
