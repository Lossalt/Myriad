//! The persona in Telegram groups: the community's first shared venue.
//!
//! She answers a group line only when it speaks to her (an @mention, a
//! mention of her, a command aimed at her, or a reply to her message), and
//! only when the sender is a paired site user — a member of the community.
//! Anyone else is silently left alone; groups get no pairing prompts.
//!
//! The turn is a Chat turn under the sender's own account (their quota), in a
//! group venue: people outside the community may be reading, so she draws
//! only on what this group has heard, never anyone's private matters (see
//! `memory::unified::Audience::group`). The group's recent lines, other
//! people's included, are the conversation she answers in; they are untrusted.
//!
//! A group gets one turn at a time and a short pause between replies, so a
//! busy group cannot crowd out everyone else. Delivery is best effort: a
//! restart mid-turn loses that reply, which is acceptable for chat.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use myriad_agent_rules::channel::{PairingLookup, TelegramGroupMessage};
use sea_orm::DatabaseConnection;
use tracing::{info, warn};

use crate::services::agent::AgentInteractionMode;
use crate::services::agent::types::{AgentProgressEvent, ConversationMessage};
use crate::services::channel_pairing::ChannelBinding;
use crate::services::channel_platform::ChannelPlatform;

/// Lines of a group she keeps in mind, and for how long.
const TRANSCRIPT_LINES: usize = 30;
const TRANSCRIPT_FOR: Duration = Duration::from_secs(6 * 3600);
const MAX_GROUPS: usize = 256;
const MAX_LINE_CHARS: usize = 500;
/// Between two of her replies in the same group.
const GROUP_PAUSE: Duration = Duration::from_secs(5);
/// Longest she takes over one group reply before giving up on it.
const TURN_DEADLINE: Duration = Duration::from_secs(90);
const TYPING_EVERY: Duration = Duration::from_secs(4);

#[derive(Clone)]
struct Line {
    at: Instant,
    message_id: Option<i64>,
    name: String,
    text: String,
    hers: bool,
}

#[derive(Default)]
struct Group {
    lines: VecDeque<Line>,
    busy: bool,
    last_reply: Option<Instant>,
    touched: Option<Instant>,
}

static GROUPS: LazyLock<Mutex<HashMap<i64, Group>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The Chat session each sender has in each group, so their turns in that
/// group supersede only each other.
static SESSIONS: LazyLock<Mutex<HashMap<(i64, i32), String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with_group<T>(chat_id: i64, act: impl FnOnce(&mut Group) -> T) -> Option<T> {
    let mut groups = GROUPS.lock().ok()?;
    if !groups.contains_key(&chat_id) && groups.len() >= MAX_GROUPS {
        if let Some(stalest) = groups
            .iter()
            .filter(|(_, group)| !group.busy)
            .min_by_key(|(_, group)| group.touched)
            .map(|(id, _)| *id)
        {
            groups.remove(&stalest);
        }
    }
    let group = groups.entry(chat_id).or_default();
    group.touched = Some(Instant::now());
    Some(act(group))
}

fn push_line(group: &mut Group, line: Line) {
    group
        .lines
        .retain(|line| line.at.elapsed() < TRANSCRIPT_FOR);
    group.lines.push_back(line);
    while group.lines.len() > TRANSCRIPT_LINES {
        group.lines.pop_front();
    }
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_LINE_CHARS).collect()
}

/// Keep a group line in mind, whoever wrote it.
pub fn record(message: &TelegramGroupMessage) {
    let line = Line {
        at: Instant::now(),
        message_id: Some(message.message_id),
        name: message.display_name.clone(),
        text: bounded(&message.text),
        hers: false,
    };
    with_group(message.chat_id, |group| push_line(group, line));
}

fn record_hers(chat_id: i64, text: &str) {
    let line = Line {
        at: Instant::now(),
        message_id: None,
        name: String::new(),
        text: bounded(text),
        hers: true,
    };
    with_group(chat_id, |group| push_line(group, line));
}

/// The group's recent lines before `message_id`, oldest first. Others' lines
/// carry their name; hers are her own turns.
fn transcript(chat_id: i64, message_id: i64) -> Vec<ConversationMessage> {
    with_group(chat_id, |group| {
        group
            .lines
            .iter()
            .filter(|line| line.at.elapsed() < TRANSCRIPT_FOR)
            .take_while(|line| line.message_id != Some(message_id))
            .map(|line| ConversationMessage {
                role: if line.hers { "assistant" } else { "user" }.into(),
                content: if line.hers {
                    line.text.clone()
                } else {
                    format!("{}：{}", line.name, line.text)
                },
                created_at: None,
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Take the group's single turn, if it is free and not just replied in.
fn begin_turn(chat_id: i64) -> bool {
    with_group(chat_id, |group| {
        let resting = group
            .last_reply
            .is_some_and(|at| at.elapsed() < GROUP_PAUSE);
        if group.busy || resting {
            return false;
        }
        group.busy = true;
        true
    })
    .unwrap_or(false)
}

fn end_turn(chat_id: i64, replied: bool) {
    with_group(chat_id, |group| {
        group.busy = false;
        if replied {
            group.last_reply = Some(Instant::now());
        }
    });
}

/// Answer one group line that spoke to her.
pub async fn handle(message: TelegramGroupMessage, token: String) {
    let chat_id = message.chat_id;
    if !begin_turn(chat_id) {
        info!(
            chat_id,
            "[Telegram group] busy or resting; line left unanswered"
        );
        return;
    }
    let replied = answer(&message, &token).await;
    end_turn(chat_id, replied);
}

async fn answer(message: &TelegramGroupMessage, token: &str) -> bool {
    let Ok(db) = crate::services::tapp_registry::database() else {
        return false;
    };
    let inbound_id = format!("group:{}:{}", message.chat_id, message.message_id);
    if !crate::services::channel_work::claim_inbound(
        &db,
        ChannelPlatform::Telegram,
        None,
        &inbound_id,
    )
    .await
    {
        return false;
    }
    let sender = message.from_id.to_string();
    // Only the community is answered; everyone else is left alone, quietly.
    let Ok(PairingLookup::Paired { user_id }) =
        crate::services::telegram_pairing::lookup_openid(&db, &sender).await
    else {
        return false;
    };
    let Some(binding) = current_binding(&db, user_id, &sender).await else {
        return false;
    };
    let Some(reply) = run_turn(&db, message, user_id, token).await else {
        return false;
    };
    // Unpaired or switched off while she was thinking: say nothing.
    if !binding.is_current(&db).await {
        return false;
    }
    let mut sent = false;
    for chunk in myriad_agent_rules::channel::split_channel_text(
        &reply,
        myriad_agent_rules::channel::TELEGRAM_TEXT_LIMIT,
    ) {
        match crate::services::telegram_bot::send_group_reply(
            token,
            message.chat_id,
            &chunk,
            message.message_id,
            message.message_thread_id,
        )
        .await
        {
            Ok(()) => sent = true,
            Err(kind) => {
                warn!(
                    ?kind,
                    chat_id = message.chat_id,
                    "[Telegram group] reply not sent"
                );
                break;
            }
        }
    }
    if sent {
        record_hers(message.chat_id, &reply);
    }
    sent
}

async fn current_binding(
    db: &DatabaseConnection,
    user_id: i32,
    sender: &str,
) -> Option<ChannelBinding> {
    let binding = ChannelBinding::resolve(db, ChannelPlatform::Telegram, user_id, sender)
        .await
        .ok()
        .flatten()?;
    binding.is_current(db).await.then_some(binding)
}

async fn run_turn(
    db: &DatabaseConnection,
    message: &TelegramGroupMessage,
    user_id: i32,
    token: &str,
) -> Option<String> {
    let claims = crate::services::channel_work::claims_for_user(db, user_id)
        .await
        .ok()?;
    let key = (message.chat_id, user_id);
    let known = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&key).cloned());
    let session_id = crate::api::agent::ensure_session(
        db,
        known.as_deref(),
        user_id,
        AgentInteractionMode::Chat,
    )
    .await
    .ok()?;
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(key, session_id.clone());
    }
    let run = crate::api::agent::start_process_run(
        db.clone(),
        claims,
        crate::api::agent::ProcessRequest {
            input: message.text.clone(),
            context: Some(crate::api::agent::ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(session_id),
                group: Some(crate::api::agent::GroupTurn {
                    venue: format!("telegram:{}", message.chat_id),
                    transcript: transcript(message.chat_id, message.message_id),
                }),
                ..Default::default()
            }),
        },
    )
    .await
    .ok()?;
    let chat = message.chat_id.to_string();
    let mut events = Box::pin(crate::api::agent::agent_run_envelopes(run));
    let mut typing = tokio::time::interval(TYPING_EVERY);
    let deadline = tokio::time::sleep(TURN_DEADLINE);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return None,
            _ = typing.tick() => {
                let _ = crate::services::telegram_bot::send_typing(token, &chat).await;
            }
            envelope = events.next() => {
                match envelope?.event {
                    AgentProgressEvent::TaskCompleted { success, response, .. } => {
                        // A superseded or failed turn says nothing in the group.
                        return success
                            .then(|| response.get("message").and_then(|value| value.as_str()))
                            .flatten()
                            .map(str::trim)
                            .filter(|text| !text.is_empty())
                            .map(str::to_string);
                    }
                    AgentProgressEvent::Error { .. } => return None,
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(chat_id: i64, message_id: i64, name: &str, text: &str) -> TelegramGroupMessage {
        TelegramGroupMessage {
            update_id: message_id,
            message_id,
            chat_id,
            message_thread_id: None,
            from_id: 1,
            display_name: name.into(),
            text: text.into(),
            addressed: false,
        }
    }

    #[test]
    fn the_group_transcript_is_the_lines_before_the_one_she_answers() {
        let chat = -9_001;
        record(&line(chat, 1, "阿明", "周五聚餐吗"));
        record(&line(chat, 2, "小红", "我可以"));
        record_hers(chat, "我在屏幕里，就不去了，你们吃好");
        record(&line(chat, 3, "阿明", "@bot 你推荐哪家"));
        let lines: Vec<(String, String)> = transcript(chat, 3)
            .into_iter()
            .map(|message| (message.role, message.content))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("user".into(), "阿明：周五聚餐吗".into()),
                ("user".into(), "小红：我可以".into()),
                ("assistant".into(), "我在屏幕里，就不去了，你们吃好".into()),
            ]
        );
    }

    #[test]
    fn a_group_gets_one_turn_at_a_time_and_a_pause_after_replying() {
        let chat = -9_002;
        assert!(begin_turn(chat));
        assert!(!begin_turn(chat), "one turn at a time");
        end_turn(chat, true);
        assert!(!begin_turn(chat), "a short pause after a reply");
        let other = -9_003;
        assert!(begin_turn(other));
        end_turn(other, false);
        assert!(begin_turn(other), "no reply, no pause");
        end_turn(other, false);
    }

    #[test]
    fn a_group_keeps_only_its_recent_lines() {
        let chat = -9_004;
        for index in 0..(TRANSCRIPT_LINES as i64 + 5) {
            record(&line(chat, index, "某人", &format!("第{index}句")));
        }
        let lines = transcript(chat, i64::MAX);
        assert_eq!(lines.len(), TRANSCRIPT_LINES);
        assert_eq!(lines[0].content, "某人：第5句");
    }
}
