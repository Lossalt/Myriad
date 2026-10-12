//! Her recent conversations with the site owner, for the app to show: the
//! `conversations` frame. They live here; the app keeps them only as a
//! display cache that goes with its link, so after a restart it shows what
//! was said here and on the site, and nothing of it is ever written there.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::connection::Outgoing;

/// The most recently active private chats sent.
pub(super) const SESSIONS: i64 = 10;
/// The last lines of each.
pub(super) const MESSAGES: i64 = 30;
/// Each line at most.
pub(super) const TEXT_CHARS: usize = 4000;
/// Room left in a frame for the envelope around the body.
const ENVELOPE_ROOM: usize = 4096;

/// One line as the app shows it.
fn line(id: i32, role: &str, content: &str, at: chrono::DateTime<chrono::FixedOffset>) -> Value {
    let text: String = content.chars().take(TEXT_CHARS).collect();
    json!({
        "id": format!("msg_{id}"),
        "role": role,
        "text": text,
        "at": at.timestamp_millis(),
    })
}

/// The owner's recent private chats with her, newest first, each with its
/// last lines oldest first: not a group's, not Work, not one they deleted.
pub(super) async fn recent(
    db: &DatabaseConnection,
    owner: i32,
) -> Result<Vec<Value>, sea_orm::DbErr> {
    let sessions = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, title, last_active_at FROM agent_sessions \
             WHERE user_id = $1 AND context->>'mode' = 'chat' AND context->>'venue' IS NULL \
               AND NOT archived AND message_count > 0 \
             ORDER BY last_active_at DESC, id LIMIT $2",
            [owner.into(), SESSIONS.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(sessions.len());
    for session in &sessions {
        let id: String = session.try_get("", "id")?;
        let title: Option<String> = session.try_get("", "title")?;
        let active: chrono::DateTime<chrono::FixedOffset> =
            session.try_get("", "last_active_at")?;
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id, role, content, created_at FROM agent_messages \
                 WHERE session_id = $1 AND role IN ('user', 'assistant') \
                 ORDER BY created_at DESC, id DESC LIMIT $2",
                [id.clone().into(), MESSAGES.into()],
            ))
            .await?;
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows.iter().rev() {
            let content: String = row.try_get("", "content")?;
            if content.trim().is_empty() {
                continue;
            }
            messages.push(line(
                row.try_get("", "id")?,
                &row.try_get::<String>("", "role")?,
                &content,
                row.try_get("", "created_at")?,
            ));
        }
        if messages.is_empty() {
            continue;
        }
        let mut entry = json!({
            "sessionId": id,
            "updatedAt": active.timestamp_millis(),
            "messages": messages,
        });
        if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
            entry["title"] = title.into();
        }
        out.push(entry);
    }
    Ok(out)
}

/// The frame, within `max_bytes`: the oldest lines go first (the oldest
/// conversation's first), and a conversation left with none goes too.
pub(super) fn frame(mut sessions: Vec<Value>, max_bytes: usize) -> Outgoing {
    let budget = max_bytes.saturating_sub(ENVELOPE_ROOM);
    // Measured once, then counted down: a line takes its JSON and a comma.
    let mut size = json!({ "sessions": &sessions }).to_string().len();
    while size > budget {
        let Some(last) = sessions.last_mut() else {
            break;
        };
        let dropped = match last["messages"].as_array_mut() {
            Some(messages) if messages.len() > 1 => Some(messages.remove(0).to_string().len() + 1),
            _ => None,
        };
        match dropped {
            Some(bytes) => size = size.saturating_sub(bytes),
            None => {
                let gone = sessions
                    .pop()
                    .map_or(0, |session| session.to_string().len() + 1);
                size = size.saturating_sub(gone);
            }
        }
    }
    Outgoing::latest("conversations", json!({ "sessions": sessions }))
}
