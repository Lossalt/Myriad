//! Words and memes she learns from the talk around her (the rules are
//! `myriad_merope::memes`): when a judgment of a group's talk names one she
//! is not sure of, her group mind (`myriad_merope::group`) looks it up in the
//! slang and meme dictionaries (or searches) and keeps what it means as her
//! own, and from then on she knows it wherever it comes up. Here: what she
//! knows, read back, and whether she may look one up now (once a day, within
//! the day's allowance for it).

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use chrono::NaiveDate;
use sea_orm::DatabaseConnection;

use crate::services::agent::memory::unified;
use myriad_merope::memes::{Known, SOURCE, known_in, read_kept, section};

#[cfg(test)]
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
/// What she knows, read back: the latest this many.
const READ_BACK: u64 = 500;

/// Words tried today, found or not, so one she could not find is not looked
/// up again all day.
static TRIED: LazyLock<Mutex<(Option<NaiveDate>, HashSet<String>)>> =
    LazyLock::new(|| Mutex::new((None, HashSet::new())));

fn first_try_today(term: &str, today: NaiveDate) -> bool {
    let Ok(mut tried) = TRIED.lock() else {
        return false;
    };
    if tried.0 != Some(today) {
        *tried = (Some(today), HashSet::new());
    }
    tried.1.insert(term.to_lowercase())
}

/// The words and memes she knows.
pub(super) async fn known(db: &DatabaseConnection) -> Vec<Known> {
    unified::own_rows(db, SOURCE, READ_BACK)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| read_kept(&row.content))
        .collect()
}

/// Those she learned since `since`, the latest `limit`.
pub(super) async fn learned_since(
    db: &DatabaseConnection,
    since: chrono::DateTime<chrono::FixedOffset>,
    limit: u64,
) -> Vec<Known> {
    unified::own_rows_since(db, SOURCE, since, limit)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| read_kept(&row.content))
        .collect()
}

/// What she knows of the words and memes `text` uses, as a prompt section.
pub async fn section_for(db: &DatabaseConnection, text: &str) -> Option<String> {
    let known = known(db).await;
    section(&known_in(text, &known))
}

/// Whether she may look `term` up now (her group mind asks, for a word a
/// judgment of the talk said she is not sure of): not tried today, found or
/// not, and within the day's allowance for words and memes.
pub(crate) async fn may_look_up(db: &DatabaseConnection, term: &str) -> myriad_merope::group::May {
    use myriad_merope::group::May;
    let today = super::clock::local_now().date_naive();
    if !first_try_today(term, today) {
        return May::Tried;
    }
    if !super::store::curiosity::claim(db, super::store::curiosity::WORDS, term, today)
        .await
        .unwrap_or(false)
    {
        tracing::info!("[Merope] no lookups left today for words and memes");
        return May::NoneLeft;
    }
    May::Yes
}

/// What was found about `term`, made into what it means and kept, if it
/// says what it means as `used` shows it used; what it means, when kept.
/// Her group mind does this itself (`myriad_merope::group`); this is the
/// same, for the memory bench.
#[cfg(test)]
pub(super) async fn keep(
    db: &DatabaseConnection,
    owner: i32,
    term: &str,
    used: &[String],
    found: &str,
    url: &str,
) -> Option<String> {
    use super::call::{self, Voice};
    use myriad_merope::memes::{
        DIGEST_SCHEMA, digest_input, digest_schema, digest_system, kept_line, parse_digest,
    };
    use serde_json::json;
    let raw = call::Ask::new(Voice::Thorough, owner, "meme")
        .within(CALL_TIMEOUT)
        .json_raw(
            &digest_system(),
            &digest_input(term, used, found),
            DIGEST_SCHEMA,
            &digest_schema(),
        )
        .await
        .ok()?;
    let Some(Some(meaning)) = parse_digest(&raw) else {
        tracing::info!("[Merope] what she found about a word does not say what it means here");
        return None;
    };
    let evidence = json!({ "term": term, "url": url }).to_string();
    let concepts = vec![unified::Concept {
        name: term.to_string(),
        aliases: Vec::new(),
    }];
    match unified::remember_own(db, &kept_line(term, &meaning), &evidence, concepts, SOURCE).await {
        Ok(Some(_)) => tracing::info!("[Merope] learned a word or meme"),
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep a word she learned"),
    }
    Some(meaning)
}
