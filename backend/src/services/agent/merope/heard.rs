//! What she heard in groups about things (see `myriad_merope::heard`): taken
//! in from a stretch of a group's talk while it is still in mind, kept as her
//! own with no one's name on it but the group's, and brought to mind in any
//! group whose talk touches it. What was about a person was never kept.

use std::time::Duration;

use myriad_merope::heard::{SCHEMA_NAME, Said, input, parse, schema, system};
use sea_orm::DatabaseConnection;
use serde_json::json;

use super::call::{self, Voice};
use crate::services::agent::memory::unified::{self, Concept};

/// Her own rows of what she heard.
pub const SOURCE: &str = "heard";
/// Heard and never brought up again, it fades.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
const HELD: u64 = 300;

/// Take in a stretch of the talk of the group `venue` (as sessions keep it);
/// the judgment is billed to `owner`.
pub async fn take_in(db: &DatabaseConnection, owner: i32, venue: &str, lines: Vec<Said>) {
    if !super::is_enabled().await || lines.iter().all(|line| line.hers) {
        return;
    }
    let Ok(raw) = call::Ask::new(Voice::Judge, owner, "heard")
        .within(CALL_TIMEOUT)
        .json_raw(system(), &input(&lines), SCHEMA_NAME, &schema())
        .await
    else {
        return;
    };
    let Some(things) = parse(&raw, &lines) else {
        return;
    };
    let held: Vec<String> = unified::own_rows(db, SOURCE, HELD)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.content)
        .collect();
    let mut kept = 0;
    for thing in things {
        if held.iter().any(|held| *held == thing) {
            continue;
        }
        let evidence = json!({ "heard": "in a group chat", "group": venue }).to_string();
        if matches!(
            unified::remember_own(db, &thing, &evidence, Vec::<Concept>::new(), SOURCE).await,
            Ok(Some(_))
        ) {
            kept += 1;
        }
    }
    if kept > 0 {
        tracing::info!(kept, "[Merope] kept what she heard in a group");
    }
    if let Err(error) = unified::fade_source(db, SOURCE, FADE_AFTER).await {
        tracing::warn!(%error, "[Merope] could not let old things she heard fade");
    }
}

/// What she heard that the talk in the group `here` (as sessions keep it)
/// brings back, by its words or its meaning, best first: where, and when.
pub async fn brought_back(
    db: &DatabaseConnection,
    here: &str,
    talk: &str,
    limit: usize,
) -> Vec<myriad_merope::heard::Recalled> {
    use myriad_merope::heard::{Recalled, Where};
    let rows = unified::own_rows(db, SOURCE, HELD)
        .await
        .unwrap_or_default();
    let now = chrono::Utc::now();
    let mut recalled = Vec::new();
    for row in unified::named_among(db, rows, talk, limit).await {
        let group = row
            .evidence
            .as_deref()
            .and_then(|evidence| serde_json::from_str::<serde_json::Value>(evidence).ok())
            .and_then(|evidence| evidence.get("group")?.as_str().map(str::to_string));
        let heard = match group {
            Some(group) if group == here => Where::Here,
            Some(group) => match crate::services::channel_group::title(&group).await {
                Some(name) => Where::In(name),
                None => Where::SomeGroup,
            },
            None => Where::SomeGroup,
        };
        recalled.push(Recalled {
            thing: row.content,
            heard,
            ago: myriad_merope::doing::ago_text(now - row.created_at.with_timezone(&chrono::Utc)),
        });
    }
    recalled
}
