//! A group's own words (see `myriad_merope::jargon`): a word she was unsure
//! of and could not look up is watched in that group; as it keeps coming up
//! she works out what it means there, beside what it usually means, and
//! keeps it as the group's when the two differ. Worked out on the site
//! owner's account, who hosts her in the groups.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use myriad_merope::jargon::{
    HERE_SCHEMA, Known, USUAL_SCHEMA, Watched, hear, here_input, here_schema, here_system,
    parse_here, parse_usual, usual_input, usual_schema, usual_system, watch,
};
use sea_orm::DatabaseConnection;
use serde_json::json;

use super::call::{self, Voice};
use crate::services::agent::memory::unified;

/// Rows of a group's own words, kept in that group.
pub const SOURCE: &str = "jargon";
const NAMESPACE: &str = "merope_jargon_watched";
/// Watched words of a group no one used for this long are let go.
const WATCHED_FOR_DAYS: i64 = 30;
/// A group's word no one brought up again in this long fades.
const FADES_AFTER: chrono::Duration = chrono::Duration::days(90);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Words of the groups read back.
const READ_BACK: u64 = 300;

/// Each group's watched words, as last read or written.
static WATCHED: LazyLock<Mutex<HashMap<String, Vec<Watched>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

/// The group `venue`'s watched words, read back once per process.
async fn watched_in(db: &DatabaseConnection, venue: &str) -> Vec<Watched> {
    if let Some(watched) = WATCHED.lock().ok().and_then(|all| all.get(venue).cloned()) {
        return watched;
    }
    let watched = crate::services::runtime_registry::get::<Vec<Watched>>(db, NAMESPACE, venue)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if let Ok(mut all) = WATCHED.lock() {
        all.entry(venue.to_string())
            .or_insert_with(|| watched.clone());
    }
    watched
}

async fn keep_watched(db: &DatabaseConnection, venue: &str, watched: Vec<Watched>) {
    if let Ok(mut all) = WATCHED.lock() {
        all.insert(venue.to_string(), watched.clone());
    }
    let until = (chrono::Utc::now() + chrono::Duration::days(WATCHED_FOR_DAYS)).timestamp();
    if let Err(error) =
        crate::services::runtime_registry::put(db, NAMESPACE, venue, identity(), &watched, until)
            .await
    {
        tracing::warn!(%error, "[Merope] could not keep the words she watches in a group");
    }
}

/// Watch `term` in the group `venue` (as sessions keep it): she was unsure
/// of it and could not look it up. `used` are lines of the talk using it;
/// hers (`you：…`) do not count.
pub(super) async fn nominate(db: &DatabaseConnection, venue: &str, term: &str, used: &[String]) {
    let theirs: Vec<String> = used
        .iter()
        .filter(|line| !line.starts_with("you："))
        .cloned()
        .collect();
    let mut watched = watched_in(db, venue).await;
    watch(&mut watched, term, &theirs);
    let due: Vec<Watched> = watched.iter().filter(|word| word.due()).cloned().collect();
    keep_watched(db, venue, watched).await;
    for word in due {
        work_out(venue.to_string(), word);
    }
}

/// A line someone else said in the group `venue`: the watched words in it
/// count, and those that came up enough are worked out.
pub fn heard(venue: &str, line: &str) {
    let has_any = WATCHED.lock().ok().is_some_and(|all| {
        all.get(venue)
            .is_some_and(|watched| watched.iter().any(|word| line.contains(&word.term)))
    });
    let known = WATCHED
        .lock()
        .ok()
        .is_some_and(|all| all.contains_key(venue));
    if known && !has_any {
        return;
    }
    let (venue, line) = (venue.to_string(), line.to_string());
    tokio::spawn(async move {
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        let mut watched = watched_in(&db, &venue).await;
        let due = hear(&mut watched, &line);
        if due.is_empty() && !watched.iter().any(|word| line.contains(&word.term)) {
            return;
        }
        let due: Vec<Watched> = due
            .into_iter()
            .map(|index| watched[index].clone())
            .collect();
        keep_watched(&db, &venue, watched).await;
        for word in due {
            work_out(venue.clone(), word);
        }
    });
}

/// Work out what `word` means in the group `venue`, in the background, and
/// keep it as the group's own when it differs from what it usually means.
fn work_out(venue: String, word: Watched) {
    super::background::spawn("a group's own words", async move {
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        let Some(owner) = super::call::site_owner().await else {
            return;
        };
        // Worked out at these uses, found or not: not asked again until more.
        let mut watched = watched_in(&db, &venue).await;
        if let Some(kept) = watched.iter_mut().find(|kept| kept.term == word.term) {
            kept.worked_out_at = kept.uses;
        }
        keep_watched(&db, &venue, watched).await;
        let usual = call::Ask::new(Voice::Judge, owner, "word_usually")
            .within(CALL_TIMEOUT)
            .json_raw(
                usual_system(),
                &usual_input(&word.term),
                USUAL_SCHEMA,
                &usual_schema(),
            )
            .await
            .ok()
            .and_then(|raw| parse_usual(&raw))
            .flatten();
        let Ok(raw) = call::Ask::new(Voice::Judge, owner, "word_here")
            .within(CALL_TIMEOUT)
            .json_raw(
                here_system(),
                &here_input(&word.term, &word.lines, usual.as_deref()),
                HERE_SCHEMA,
                &here_schema(),
            )
            .await
        else {
            return;
        };
        let Some(Some(meaning)) = parse_here(&raw) else {
            tracing::info!(%venue, "[Merope] a word she watched is not the group's own");
            return;
        };
        keep(&db, &venue, &word, &meaning).await;
    });
}

/// Keep what `word` means in the group `venue`, in place of what she made of
/// it before.
async fn keep(db: &DatabaseConnection, venue: &str, word: &Watched, meaning: &str) {
    let stored = unified::Audience::group(venue, 0).venue();
    let marker = format!("\"term\":{}", serde_json::Value::String(word.term.clone()));
    let before = unified::unowned_with_evidence(db, &stored, SOURCE, &marker)
        .await
        .ok()
        .flatten();
    let evidence = json!({ "term": word.term, "uses": word.uses }).to_string();
    let content = myriad_merope::memes::kept_line(&word.term, meaning);
    match unified::remember_in_venue(db, &stored, &content, &evidence, SOURCE).await {
        Ok(Some(id)) => {
            if let Some(before) = before.filter(|before| before.id != id) {
                let _ = unified::retire_unowned_in_favor_of(
                    db,
                    &stored,
                    &before.id,
                    "superseded",
                    Some(&id),
                )
                .await;
            }
            tracing::info!(%venue, "[Merope] worked out a word of the group's own");
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep a group's word"),
    }
    if let Err(error) = unified::fade_source(db, SOURCE, FADES_AFTER).await {
        tracing::warn!(%error, "[Merope] could not let old group words fade");
    }
}

/// The groups' own words `text`, said in the group `here` (as sessions keep
/// it), uses: this group's, and other groups' by their names.
pub async fn section_for(db: &DatabaseConnection, here: &str, text: &str) -> Option<String> {
    let rows = unified::in_other_groups(db, None, "", &[SOURCE], READ_BACK).await;
    let here_stored = unified::Audience::group(here, 0).venue();
    let mut known = Vec::new();
    for row in rows {
        let Some(word) = myriad_merope::memes::read_kept(&row.content) else {
            continue;
        };
        if !text.contains(&word.term) {
            continue;
        }
        let (group, elsewhere) = if row.venue == here_stored {
            (None, false)
        } else {
            let venue = row.venue.strip_prefix("group:").unwrap_or(&row.venue);
            (crate::services::channel_group::title(venue).await, true)
        };
        known.push(Known {
            term: word.term,
            meaning: word.meaning,
            group,
            elsewhere,
        });
    }
    myriad_merope::jargon::section(&known)
}
