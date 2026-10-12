//! Her recent conversations with the site owner, for the app to show: the
//! `conversations` frame. They live here; the app keeps them only as a
//! display cache that goes with its link, so after a restart it shows what
//! was said here and on the site, and nothing of it is ever written there.
//!
//! A child that also takes `conversations.live` (D37) is shown her lines
//! said unprompted outside a chat too, each in the conversation that was the
//! latest when she said it, and is kept current: each change to one of
//! these conversations comes as `conversation` while its link is open.

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

/// Her lines said unprompted, at most this many of the newest.
pub(super) const SAID: i64 = 30;
/// The query that keeps a live child current: the recent conversations
/// and her lines, at the cost of one statement.
const CHANGES: &str = "SELECT coalesce((SELECT string_agg(s.id || ':' || s.message_count || ':' || coalesce(s.title, '') || ':' || \
       (extract(epoch FROM s.last_active_at) * 1000)::bigint, ',' ORDER BY s.last_active_at DESC, s.id) \
     FROM (SELECT id, message_count, title, last_active_at FROM agent_sessions \
       WHERE user_id = $1 AND context->>'mode' = 'chat' AND context->>'venue' IS NULL \
         AND NOT archived AND message_count > 0 \
       ORDER BY last_active_at DESC, id LIMIT $2) s), '') || '|' || \
   coalesce((SELECT max(id) FROM agent_proactive_messages WHERE user_id = $1), 0) || '|' || \
   coalesce((SELECT max(m.id) FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
     WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL), 0) AS changes";

/// The owner's recent private chats with her, newest first, each with its
/// last lines oldest first: not a group's, not Work, not one they deleted.
#[cfg(test)]
pub(super) async fn recent(
    db: &DatabaseConnection,
    owner: i32,
) -> Result<Vec<Value>, sea_orm::DbErr> {
    recent_with(db, owner, false).await
}

/// What the recent conversations are now, as cheaply as it can be told:
/// it changes whenever one of them, or her lines, does.
pub(super) async fn changes(db: &DatabaseConnection, owner: i32) -> Result<String, sea_orm::DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            CHANGES,
            [owner.into(), SESSIONS.into()],
        ))
        .await?;
    Ok(row
        .and_then(|row| row.try_get::<String>("", "changes").ok())
        .unwrap_or_default())
}

/// Her lines said unprompted, newest first: (id, text, at, say id).
async fn said(
    db: &DatabaseConnection,
    owner: i32,
) -> Result<
    Vec<(
        i64,
        String,
        chrono::DateTime<chrono::FixedOffset>,
        Option<String>,
    )>,
    sea_orm::DbErr,
> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, content, created_at, say_id FROM agent_proactive_messages \
             WHERE user_id = $1 AND role = 'assistant' ORDER BY created_at DESC, id DESC LIMIT $2",
            [owner.into(), SAID.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let content: String = row.try_get("", "content")?;
        if content.trim().is_empty() {
            continue;
        }
        out.push((
            row.try_get("", "id")?,
            content,
            row.try_get("", "created_at")?,
            row.try_get("", "say_id")?,
        ));
    }
    Ok(out)
}

/// Her line said unprompted, as a line of a conversation.
fn said_line(
    id: i64,
    content: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
    say_id: Option<&str>,
) -> Value {
    let id = say_id
        .map(|say| format!("say_{say}"))
        .filter(|id| super::contract::valid_id(id))
        .unwrap_or_else(|| format!("pro_{id}"));
    let text: String = content.chars().take(TEXT_CHARS).collect();
    json!({ "id": id, "role": "assistant", "text": text, "at": at.timestamp_millis(), "kind": "proactive" })
}

/// Puts each of her lines into the conversation that was the latest when
/// she said it: the one with the newest line at or before it, else (said
/// before all of them) the one that began first. Lines older than every
/// conversation shown stay out.
pub(super) fn place_said(sessions: &mut [Value], said: Vec<Value>) {
    let at = |line: &Value| line["at"].as_i64().unwrap_or(0);
    let oldest = sessions
        .iter()
        .filter_map(|session| session["messages"].as_array()?.first().map(at))
        .min();
    let Some(oldest) = oldest else {
        return;
    };
    for line in said {
        let when = at(&line);
        if when < oldest {
            continue;
        }
        let latest = sessions
            .iter()
            .enumerate()
            .filter_map(|(index, session)| {
                let before = session["messages"]
                    .as_array()?
                    .iter()
                    .filter(|message| message["kind"] != "proactive")
                    .map(at)
                    .filter(|line_at| *line_at <= when)
                    .max()?;
                Some((before, index))
            })
            .max();
        let index = match latest {
            Some((_, index)) => index,
            None => sessions
                .iter()
                .enumerate()
                .filter_map(|(index, session)| {
                    Some((session["messages"].as_array()?.first().map(at)?, index))
                })
                .min()
                .map_or(0, |(_, index)| index),
        };
        if let Some(messages) = sessions[index]["messages"].as_array_mut() {
            messages.push(line);
            messages.sort_by_key(|message| message["at"].as_i64().unwrap_or(0));
        }
    }
}

/// The recent conversations; with `said`, her lines said unprompted placed in them.
pub(super) async fn recent_with(
    db: &DatabaseConnection,
    owner: i32,
    with_said: bool,
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
    if with_said {
        let lines = said(db, owner)
            .await?
            .into_iter()
            .rev()
            .map(|(id, content, at, say_id)| said_line(id, &content, at, say_id.as_deref()))
            .collect();
        place_said(&mut out, lines);
    }
    Ok(out)
}

/// The `conversation` frame: one of them as it stands now, within
/// `max_bytes` as [`frame`] keeps it (its oldest lines go first).
pub(super) fn one_frame(session: Value, max_bytes: usize) -> Option<Outgoing> {
    let mut kept = frame(vec![session], max_bytes).body;
    let session = kept["sessions"].as_array_mut()?.pop()?;
    Some(Outgoing::latest(
        "conversation",
        json!({ "session": session }),
    ))
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
