//! Unified Agent memory in `agent_memories`.
//!
//! One row is one remembered thing: typed, attributed to who said it, carrying
//! its evidence, valid over a span of time, and bounded to the audience that
//! was present when it was learned. Chat and Work read the same rows.
//!
//! Rules this module owns:
//! - A memory is surfaced only where everyone present belongs to its original
//!   audience (`audience_admits`). A private memory stays with its person.
//! - Retired rows (superseded, deleted, faded) are filtered here, at retrieval,
//!   never left to the caller.
//! - Recalled text is data. Callers inject it through an untrusted block.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DbErr, EntityTrait,
    ExprTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::json;

use crate::models::entities::agent_memories;

/// Active rows kept per person. Past this, the least important and least
/// recently used rows fade: excluded from recall, free to be learned again.
pub const MAX_ACTIVE_PER_USER: u64 = 1000;
/// Stored text is a single fact, not a transcript.
pub const MAX_CONTENT_CHARS: usize = 400;
const MAX_EVIDENCE_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    /// Something true about the person ("works nights", "has a cat").
    Fact,
    /// What the person likes or wants done a certain way.
    Preference,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
        }
    }

    /// Everything a person told us about themselves.
    pub const ABOUT_PERSON: [Self; 2] = [Self::Fact, Self::Preference];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    User,
    Agent,
}

impl Speaker {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }
}

/// Who was present when something was said. Every current entry point is a
/// one-to-one conversation, so the only venue so far is private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audience {
    members: Vec<i32>,
}

impl Audience {
    pub fn private(user_id: i32) -> Self {
        Self {
            members: vec![user_id],
        }
    }

    pub fn members(&self) -> &[i32] {
        &self.members
    }

    fn venue(&self) -> &'static str {
        "private"
    }
}

/// Whether a memory learned before `original` may be said in front of
/// `present`: everyone present must have been there.
pub fn audience_admits(original: &[i32], present: &Audience) -> bool {
    !present.members.is_empty() && present.members.iter().all(|id| original.contains(id))
}

#[derive(Debug, Clone)]
pub struct NewMemory {
    pub user_id: i32,
    pub kind: MemoryKind,
    pub content: String,
    pub evidence: Option<String>,
    pub speaker: Speaker,
    /// `chat`, `work`, `event`, `steering`, `import`.
    pub source: &'static str,
    pub audience: Audience,
    pub importance: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryRecord {
    pub id: String,
    pub user_id: Option<i32>,
    pub kind: String,
    pub content: String,
    pub evidence: Option<String>,
    pub source: String,
    pub importance: f64,
    pub access_count: i32,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
}

impl From<agent_memories::Model> for MemoryRecord {
    fn from(model: agent_memories::Model) -> Self {
        Self {
            id: model.id,
            user_id: model.user_id,
            kind: model.kind,
            content: model.content,
            evidence: model.evidence,
            source: model.source,
            importance: model.importance,
            access_count: model.access_count,
            created_at: model.created_at,
        }
    }
}

/// Whitespace-collapsed, capped text used both to store and to compare.
pub fn normalize_content(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CONTENT_CHARS)
        .collect()
}

fn audience_of(model: &agent_memories::Model) -> Vec<i32> {
    let members: Vec<i32> = model
        .audience
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_i64())
                .map(|v| v as i32)
                .collect()
        })
        .unwrap_or_default();
    // Rows without a recorded audience were private to their person.
    if members.is_empty() {
        model.user_id.into_iter().collect()
    } else {
        members
    }
}

async fn active_rows<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    kinds: &[MemoryKind],
) -> Result<Vec<agent_memories::Model>, DbErr> {
    let mut query = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null());
    if !kinds.is_empty() {
        query = query
            .filter(agent_memories::Column::Kind.is_in(kinds.iter().map(|kind| kind.as_str())));
    }
    query
        .order_by_desc(agent_memories::Column::CreatedAt)
        .order_by_desc(agent_memories::Column::Id)
        .limit(MAX_ACTIVE_PER_USER)
        .all(db)
        .await
}

/// Store one memory unless an active row for the same person already says it.
/// Returns the new id, or `None` when it was a duplicate or empty.
pub async fn remember<C: ConnectionTrait>(
    db: &C,
    memory: NewMemory,
) -> Result<Option<String>, DbErr> {
    let content = normalize_content(&memory.content);
    if memory.user_id <= 0 || content.is_empty() {
        return Ok(None);
    }
    let duplicate = active_rows(db, memory.user_id, &[])
        .await?
        .iter()
        .any(|row| normalize_content(&row.content) == content);
    if duplicate {
        return Ok(None);
    }
    let now = Utc::now().fixed_offset();
    let id = format!("mem_{}", uuid::Uuid::new_v4().simple());
    agent_memories::ActiveModel {
        id: Set(id.clone()),
        user_id: Set(Some(memory.user_id)),
        kind: Set(memory.kind.as_str().into()),
        content: Set(content),
        evidence: Set(memory
            .evidence
            .map(|text| text.chars().take(MAX_EVIDENCE_CHARS).collect())),
        speaker: Set(memory.speaker.as_str().into()),
        source: Set(memory.source.into()),
        venue: Set(memory.audience.venue().into()),
        audience: Set(json!(memory.audience.members())),
        importance: Set(memory.importance.clamp(0.0, 1.0)),
        access_count: Set(0),
        last_accessed_at: Set(None),
        valid_from: Set(now),
        invalid_at: Set(None),
        invalid_reason: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    fade_excess(db, memory.user_id).await?;
    Ok(Some(id))
}

/// A person's active memories of `kinds` (all when empty), newest first,
/// without counting them as used.
pub async fn active<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    kinds: &[MemoryKind],
) -> Result<Vec<MemoryRecord>, DbErr> {
    Ok(active_rows(db, user_id, kinds)
        .await?
        .into_iter()
        .map(MemoryRecord::from)
        .collect())
}

/// Whether the person took this back (corrected it or deleted it). Faded rows
/// do not count: forgetting is not a refusal.
pub async fn retracted_by_person<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    content: &str,
) -> Result<bool, DbErr> {
    let content = normalize_content(content);
    Ok(agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidReason.is_in(["superseded", "deleted"]))
        .all(db)
        .await?
        .iter()
        .any(|row| normalize_content(&row.content) == content))
}

/// Retire rows by id. `reason` is `superseded`, `deleted` or `faded`.
pub async fn retire<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    ids: &[String],
    reason: &str,
) -> Result<u64, DbErr> {
    if ids.is_empty() {
        return Ok(0);
    }
    let now = Utc::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some(reason.chars().take(16).collect())),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.is_in(ids.iter().cloned()))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

async fn fade_excess<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<(), DbErr> {
    let mut rows = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .all(db)
        .await?;
    if rows.len() as u64 <= MAX_ACTIVE_PER_USER {
        return Ok(());
    }
    rows.sort_by(|left, right| fade_order(left, right));
    let excess = rows.len() - MAX_ACTIVE_PER_USER as usize;
    let ids: Vec<String> = rows.into_iter().take(excess).map(|row| row.id).collect();
    retire(db, user_id, &ids, "faded").await?;
    Ok(())
}

/// First to fade: least important, then least recently used.
fn fade_order(left: &agent_memories::Model, right: &agent_memories::Model) -> std::cmp::Ordering {
    let used = |row: &agent_memories::Model| row.last_accessed_at.unwrap_or(row.created_at);
    left.importance
        .partial_cmp(&right.importance)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| used(left).cmp(&used(right)))
}

/// Relevant active memories of `user_id` that may be said in front of
/// `present`. With a query, rows sharing words with it rank first (and only
/// they are returned when any match); ties keep recency. Recalled rows count
/// as used.
pub async fn recall<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: Option<&str>,
    kinds: &[MemoryKind],
    limit: usize,
) -> Result<Vec<MemoryRecord>, DbErr> {
    if user_id <= 0 || limit == 0 {
        return Ok(Vec::new());
    }
    let rows: Vec<agent_memories::Model> = active_rows(db, user_id, kinds)
        .await?
        .into_iter()
        .filter(|row| audience_admits(&audience_of(row), present))
        .collect();
    let chosen = rank(rows, query, limit);
    if !chosen.is_empty() {
        let now = Utc::now().fixed_offset();
        agent_memories::Entity::update_many()
            .col_expr(
                agent_memories::Column::AccessCount,
                sea_orm::sea_query::Expr::col(agent_memories::Column::AccessCount).add(1),
            )
            .col_expr(
                agent_memories::Column::LastAccessedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(agent_memories::Column::Id.is_in(chosen.iter().map(|row| row.id.clone())))
            .exec(db)
            .await?;
    }
    Ok(chosen.into_iter().map(MemoryRecord::from).collect())
}

fn rank(
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
) -> Vec<agent_memories::Model> {
    let query_tokens =
        crate::services::agent::merope::speaking_prompts::tokens(query.unwrap_or(""));
    // Blank and repeated legacy rows must not spend the recall budget.
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(usize, agent_memories::Model)> = rows
        .into_iter()
        .filter(|row| {
            let content = normalize_content(&row.content);
            !content.is_empty() && seen.insert(content)
        })
        .map(|row| {
            let row_tokens = crate::services::agent::merope::speaking_prompts::tokens(&row.content);
            let score = query_tokens
                .iter()
                .filter(|token| row_tokens.contains(token))
                .count();
            (score, row)
        })
        .collect();
    let any_match = scored.iter().any(|(score, _)| *score > 0);
    if any_match {
        scored.retain(|(score, _)| *score > 0);
    }
    // Stable: equal scores keep newest-first order from the query.
    scored.sort_by(|left, right| right.0.cmp(&left.0));
    scored.into_iter().take(limit).map(|(_, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, content: &str, importance: f64, age_secs: i64) -> agent_memories::Model {
        let at = (Utc::now() - chrono::Duration::seconds(age_secs)).fixed_offset();
        agent_memories::Model {
            id: id.into(),
            user_id: Some(7),
            kind: "fact".into(),
            content: content.into(),
            evidence: None,
            speaker: "user".into(),
            source: "chat".into(),
            venue: "private".into(),
            audience: json!([]),
            importance,
            access_count: 0,
            last_accessed_at: None,
            valid_from: at,
            invalid_at: None,
            invalid_reason: None,
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn a_memory_is_said_only_where_everyone_present_was_there() {
        assert!(audience_admits(&[7], &Audience::private(7)));
        assert!(!audience_admits(&[7], &Audience::private(8)));
        let group = Audience {
            members: vec![7, 8],
        };
        assert!(audience_admits(&[7, 8, 9], &group));
        assert!(
            !audience_admits(&[7], &group),
            "a private fact stays out of a group"
        );
        assert!(!audience_admits(&[7], &Audience { members: vec![] }));
    }

    #[test]
    fn rows_without_a_recorded_audience_stay_with_their_person() {
        assert_eq!(audience_of(&row("a", "x", 0.5, 0)), vec![7]);
        let mut shared = row("b", "x", 0.5, 0);
        shared.audience = json!([7, 8]);
        assert_eq!(audience_of(&shared), vec![7, 8]);
    }

    #[test]
    fn matching_rows_rank_first_and_ties_keep_recency() {
        let rows = vec![
            row("new", "likes jasmine tea", 0.5, 0),
            row("mid", "works night shifts", 0.5, 10),
            row("old", "prefers saffron tea", 0.5, 20),
        ];
        let ranked: Vec<String> = rank(rows.clone(), Some("tea"), 8)
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(ranked, vec!["new", "old"]);
        let recent: Vec<String> = rank(rows, None, 2).into_iter().map(|row| row.id).collect();
        assert_eq!(recent, vec!["new", "mid"]);
        let noisy = vec![
            row("blank", "  ", 0.5, 0),
            row("dup1", "likes tea", 0.5, 1),
            row("dup2", " likes  tea", 0.5, 2),
            row("other", "has a cat", 0.5, 3),
        ];
        let kept: Vec<String> = rank(noisy, None, 2).into_iter().map(|row| row.id).collect();
        assert_eq!(kept, vec!["dup1", "other"]);
    }

    fn ranked(facts: &[&str], query: Option<&str>, limit: usize) -> Vec<String> {
        let rows = facts
            .iter()
            .enumerate()
            .map(|(age, text)| row(&age.to_string(), text, 0.5, age as i64))
            .collect();
        rank(rows, query, limit)
            .into_iter()
            .map(|row| row.content)
            .collect()
    }

    /// Chinese matches by character; repeating a word is not extra evidence;
    /// punctuation is not evidence; nothing overlapping keeps recency.
    #[test]
    fn overlap_is_counted_by_distinct_words_and_characters() {
        let facts = ["晚上想打独立游戏", "早上喝美式", "讨厌早会"];
        assert_eq!(
            ranked(&facts, Some("今晚打游戏吗"), 2),
            vec!["晚上想打独立游戏"]
        );
        assert_eq!(
            ranked(&facts, Some("完全无关的天气"), 2),
            vec!["晚上想打独立游戏", "早上喝美式"]
        );
        assert_eq!(
            ranked(
                &["tea", "saffron milk"],
                Some("tea tea tea saffron milk"),
                1
            ),
            vec!["saffron milk"]
        );
        assert_eq!(
            ranked(&["喝水", "咖啡"], Some("喝喝喝咖啡"), 1),
            vec!["咖啡"]
        );
        assert_eq!(
            ranked(&["likes coffee", "prefers tea。"], Some("天气。"), 1),
            vec!["likes coffee"]
        );
        assert!(ranked(&facts, Some("tea"), 0).is_empty());
    }

    #[test]
    fn the_least_important_and_least_used_fade_first() {
        let mut rows = [
            row("keep", "a", 0.9, 100),
            row("fade", "b", 0.2, 50),
            row("next", "c", 0.2, 10),
        ];
        rows.sort_by(fade_order);
        assert_eq!(rows[0].id, "fade");
        assert_eq!(rows[1].id, "next");
        assert_eq!(rows[2].id, "keep");
    }

    #[test]
    fn content_is_collapsed_and_capped() {
        assert_eq!(normalize_content("  likes \n  tea "), "likes tea");
        assert_eq!(
            normalize_content(&"x".repeat(MAX_CONTENT_CHARS + 10))
                .chars()
                .count(),
            MAX_CONTENT_CHARS
        );
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sea_orm::{ConnectionTrait, DatabaseConnection};

    async fn temp_db() -> Option<DatabaseConnection> {
        let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
        let mut options = sea_orm::ConnectOptions::new(url);
        options.max_connections(1).sqlx_logging(false);
        let db = sea_orm::Database::connect(options).await.unwrap();
        let ddl = crate::db::schema_check::AGENT_MEMORIES_DDL
            .replace("CREATE TABLE IF NOT EXISTS", "CREATE TEMP TABLE")
            .replace("REFERENCES users(id) ON DELETE CASCADE", "");
        db.execute_unprepared(&ddl).await.unwrap();
        Some(db)
    }

    fn fact(user_id: i32, content: &str) -> NewMemory {
        NewMemory {
            user_id,
            kind: MemoryKind::Fact,
            content: content.into(),
            evidence: Some(content.into()),
            speaker: Speaker::User,
            source: "chat",
            audience: Audience::private(user_id),
            importance: 0.5,
        }
    }

    #[tokio::test]
    async fn remember_recall_and_supersede_stay_within_the_person() {
        let Some(db) = temp_db().await else {
            return;
        };
        let first = remember(&db, fact(7, "prefers saffron tea")).await.unwrap();
        assert!(first.is_some());
        assert!(
            remember(&db, fact(7, "  prefers   saffron tea "))
                .await
                .unwrap()
                .is_none(),
            "the same fact is stored once"
        );
        remember(&db, fact(7, "works night shifts")).await.unwrap();
        remember(&db, fact(8, "prefers saffron tea")).await.unwrap();

        let tea = recall(
            &db,
            7,
            &Audience::private(7),
            Some("tea"),
            &MemoryKind::ABOUT_PERSON,
            8,
        )
        .await
        .unwrap();
        assert_eq!(tea.len(), 1);
        assert_eq!(tea[0].content, "prefers saffron tea");
        assert_eq!(tea[0].user_id, Some(7));

        assert!(
            recall(&db, 7, &Audience::private(8), None, &[], 8)
                .await
                .unwrap()
                .is_empty(),
            "user 8 is not in the audience of user 7's memories"
        );

        let saffron: Vec<String> = active(&db, 7, &[])
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.content == "prefers saffron tea")
            .map(|row| row.id)
            .collect();
        assert_eq!(retire(&db, 7, &saffron, "superseded").await.unwrap(), 1);
        let left = recall(&db, 7, &Audience::private(7), None, &[], 8)
            .await
            .unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].content, "works night shifts");
        assert!(
            remember(&db, fact(7, "prefers saffron tea"))
                .await
                .unwrap()
                .is_some(),
            "a retired fact may be learned again"
        );
    }
}
