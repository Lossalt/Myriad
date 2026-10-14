//! What her group mind reads and keeps, read and kept here: plain reads of
//! memories (which rows, newest first, how many), their cues and vectors;
//! what comes to mind of them is worked out in the core.

use myriad_merope::group::{Answer, MemoryRow, Newest, ToRead, ToWrite};

use super::*;
use crate::services::agent::memory::unified;

fn row(model: crate::models::entities::agent_memories::Model) -> MemoryRow {
    MemoryRow {
        concepts: serde_json::from_value(model.concepts.clone()).unwrap_or_default(),
        id: model.id,
        content: model.content,
        source: model.source,
        evidence: model.evidence,
        created_at: model.created_at.timestamp_micros(),
        access_count: model.access_count,
        last_accessed_at: model.last_accessed_at.map(|at| at.timestamp_micros()),
    }
}

/// A recording without a database of its own reads nothing kept and keeps
/// nothing (what it would have kept is written down instead).
fn no_database() -> bool {
    #[cfg(test)]
    if g0::on() && g0::database().is_none() {
        return true;
    }
    false
}

pub(super) async fn read(venue: &str, what: ToRead) -> Answer {
    let _ = venue;
    if no_database() && !matches!(what, ToRead::Soul) {
        return match what {
            ToRead::Cues { .. } => Answer::Cues {
                cues: Default::default(),
            },
            ToRead::Vectors { .. } => Answer::Vectors {
                vectors: Vec::new(),
                known: false,
            },
            ToRead::Life { .. } => Answer::Life {
                did: Vec::new(),
                would_tell: Vec::new(),
                now: None,
            },
            _ => Answer::Rows { rows: Vec::new() },
        };
    }
    match what {
        ToRead::Soul => Answer::Soul { text: soul().await },
        ToRead::VenueRows {
            venue,
            source,
            newest,
            limit,
        } => {
            let Ok(db) = db() else {
                return Answer::Rows { rows: Vec::new() };
            };
            let rows = match newest {
                Newest::Updated => {
                    unified::venue_source_rows(&db, None, &venue, &source, limit).await
                }
                Newest::Created => unified::latest_in_venue(&db, &venue, &source, limit).await,
            };
            Answer::Rows {
                rows: rows.unwrap_or_default().into_iter().map(row).collect(),
            }
        }
        ToRead::OwnRows { source, limit } => {
            let (Ok(db), Some(source)) = (db(), own_source(&source)) else {
                return Answer::Rows { rows: Vec::new() };
            };
            Answer::Rows {
                rows: unified::own_rows(&db, source, limit)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(row)
                    .collect(),
            }
        }
        // What her life on the site says: the facts, as her life keeps
        // them.
        ToRead::Life { talk, related } => {
            use crate::services::agent::merope::group::doing;
            let Ok(db) = db() else {
                return Answer::Life {
                    did: Vec::new(),
                    would_tell: Vec::new(),
                    now: None,
                };
            };
            Answer::Life {
                did: doing::recalled(&db, Some(&talk), 0, related).await,
                would_tell: doing::would_tell(&db, None).await,
                now: doing::now_text(crate::services::agent::memory::utc_now()),
            }
        }
        ToRead::Cues { ids } => {
            let cues = match db() {
                Ok(db) => unified::cues_of(&db, &ids).await,
                Err(_) => Default::default(),
            };
            Answer::Cues { cues }
        }
        ToRead::Vectors { rows, fill } => {
            let rows: Vec<crate::services::agent::memory::meaning::Memo> = rows
                .into_iter()
                .map(|(id, content)| crate::services::agent::memory::meaning::Memo { id, content })
                .collect();
            let vectors = match db() {
                Ok(db) => {
                    crate::services::agent::memory::meaning::vectors_of(&db, &rows, fill).await
                }
                Err(_) => None,
            };
            Answer::Vectors {
                known: vectors.is_some(),
                vectors: vectors
                    .map(|vectors| {
                        // In the rows' order, so the answer reads the same
                        // every time.
                        rows.iter()
                            .filter_map(|row| Some((row.id.clone(), vectors.get(&row.id)?.clone())))
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        }
    }
}

/// Her own sources her group mind reads.
fn own_source(source: &str) -> Option<&'static str> {
    match source {
        "view" => Some(unified::OWN_VIEW),
        "heard" => Some(crate::services::agent::merope::group::heard::SOURCE),
        "meme" => Some(myriad_merope::memes::SOURCE),
        _ => None,
    }
}

/// Sources kept in a venue that her group mind may keep.
fn source(source: &str) -> Option<&'static str> {
    match source {
        "about_me" => Some("about_me"),
        _ => None,
    }
}

pub(super) async fn write(what: ToWrite) {
    #[cfg(test)]
    if no_database() {
        let ToWrite::RememberInVenue {
            venue,
            content,
            evidence,
            ..
        } = what;
        let line = serde_json::from_str::<serde_json::Value>(&evidence)
            .ok()
            .and_then(|evidence| evidence["line"].as_str().map(str::to_string))
            .unwrap_or_default();
        g0::note(
            "told",
            serde_json::json!({ "venue": venue, "told": content, "line": line }),
        );
        return;
    }
    match what {
        ToWrite::RememberInVenue {
            venue,
            content,
            evidence,
            source: kept_as,
        } => {
            let (Ok(db), Some(kept_as)) = (db(), source(&kept_as)) else {
                return;
            };
            match unified::remember_in_venue(&db, &venue, &content, &evidence, kept_as).await {
                Ok(Some(_)) => {
                    info!(%venue, "[Merope] kept what the group told her about herself")
                }
                Ok(None) => {}
                Err(error) => warn!(%error, "[Merope] could not keep what she was told"),
            }
        }
    }
}
