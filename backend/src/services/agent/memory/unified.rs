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
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::models::entities::agent_memories;

/// Active rows kept per person. Past this, the least important and least
/// recently used rows fade: excluded from recall, free to be learned again.
pub const MAX_ACTIVE_PER_USER: u64 = 1000;
/// Stored text is a single fact, not a transcript.
pub const MAX_CONTENT_CHARS: usize = 400;
const MAX_EVIDENCE_CHARS: usize = 400;
/// A memory is about a few things, not a topic list.
pub const MAX_CONCEPTS: usize = 5;
pub const MAX_ALIASES: usize = 5;
const MAX_CONCEPT_CHARS: usize = 24;
/// Share of a recalled memory's rank that comes from being named directly;
/// the rest is its activation after spreading.
const DIRECT_WEIGHT: f64 = 0.6;
/// Activation an unnamed memory needs before it comes to mind at all.
const ASSOCIATED_MIN: f64 = 0.15;
/// Share of a memory's activation still there one turn later.
const PRIMING_FADE: f64 = 0.5;
/// How many memories stay on the mind between turns.
const PRIMING_KEPT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    /// Something true about the person ("works nights", "has a cat").
    Fact,
    /// What the person likes or wants done a certain way.
    Preference,
    /// How a kind of task went wrong and what to do instead.
    Lesson,
    /// A way of doing a task that worked.
    Pattern,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Lesson => "lesson",
            Self::Pattern => "pattern",
        }
    }

    /// Everything a person told us about themselves.
    pub const ABOUT_PERSON: [Self; 2] = [Self::Fact, Self::Preference];
    /// How Work should go about things for this person.
    pub const FOR_WORK: [Self; 4] = [Self::Preference, Self::Fact, Self::Lesson, Self::Pattern];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    User,
    Agent,
    /// Carried over from the pre-unified stores; who said it is unknown.
    Import,
}

impl Speaker {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Import => "import",
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

/// Something a memory is about, with the other names people use for it, so
/// "喵" finds the memory about the cat. Written by the same model call that
/// wrote the memory; the aliases are that model's knowledge, not the person's
/// words, and are used only to match, never shown as something they said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl Concept {
    /// Every name this concept answers to, the canonical one first.
    pub fn surface_forms(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.aliases.iter().map(String::as_str))
    }
}

fn clean_name(text: &str) -> Option<String> {
    let text: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CONCEPT_CHARS)
        .collect();
    // A lone Latin letter or digit would match inside almost anything.
    let meaningful = text.chars().filter(|ch| ch.is_alphanumeric()).count() >= 2
        || text
            .chars()
            .any(|ch| ch.is_alphanumeric() && !ch.is_ascii());
    meaningful.then_some(text)
}

/// Trim, cap and de-duplicate model-written concepts. Order is kept.
pub fn clean_concepts(raw: Vec<Concept>) -> Vec<Concept> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for concept in raw {
        let Some(name) = clean_name(&concept.name) else {
            continue;
        };
        if !seen.insert(name.to_lowercase()) {
            continue;
        }
        let mut aliases: Vec<String> = Vec::new();
        for alias in concept.aliases.iter().filter_map(|alias| clean_name(alias)) {
            let key = alias.to_lowercase();
            if key != name.to_lowercase() && !aliases.iter().any(|kept| kept.to_lowercase() == key)
            {
                aliases.push(alias);
            }
            if aliases.len() == MAX_ALIASES {
                break;
            }
        }
        out.push(Concept { name, aliases });
        if out.len() == MAX_CONCEPTS {
            break;
        }
    }
    out
}

fn concepts_of(model: &agent_memories::Model) -> Vec<Concept> {
    serde_json::from_value(model.concepts.clone()).unwrap_or_default()
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
    pub concepts: Vec<Concept>,
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
        concepts: Set(json!(clean_concepts(memory.concepts))),
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

/// Activation left over from recent turns, by memory id: what was on the
/// person's mind a moment ago. It seeds the next recall, so a topic carries
/// over a turn that does not name it ("它又吐了" after talking about the cat),
/// and fades within a few turns unless the talk keeps it alive.
///
/// Only ids are kept, never text. A memory retired or no longer admitted for
/// the present audience in the meantime is simply not among the rows it can
/// seed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Priming {
    by_id: std::collections::HashMap<String, f64>,
}

impl Priming {
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    fn of(&self, id: &str) -> f64 {
        self.by_id.get(id).copied().unwrap_or(0.0)
    }

    #[cfg(test)]
    pub(crate) fn with(id: &str, activation: f64) -> Self {
        Self {
            by_id: [(id.to_string(), activation)].into(),
        }
    }
}

/// Relevant active memories of `user_id` that may be said in front of
/// `present`. With a query, rows it truly names (BM25, see `lexical`) come to
/// mind first, then what they bring along by association (see
/// `association`). When nothing is named, recency. Recalled rows count as
/// used.
pub async fn recall<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: Option<&str>,
    kinds: &[MemoryKind],
    limit: usize,
) -> Result<Vec<MemoryRecord>, DbErr> {
    let priming = Priming::default();
    let (recalled, _) =
        recall_primed(db, user_id, present, query, kinds, limit, &priming, 1.0).await?;
    Ok(recalled)
}

/// [`recall`] that also starts from what was active a moment ago, and returns
/// what is active now for the next turn. `breadth` (`0..=1`) is how far
/// thought may wander: the share of the budget association may fill, from
/// none (only what was named) to half.
#[allow(clippy::too_many_arguments)]
pub async fn recall_primed<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: Option<&str>,
    kinds: &[MemoryKind],
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Vec<MemoryRecord>, Priming), DbErr> {
    if user_id <= 0 || limit == 0 {
        return Ok((Vec::new(), Priming::default()));
    }
    let rows: Vec<agent_memories::Model> = active_rows(db, user_id, kinds)
        .await?
        .into_iter()
        .filter(|row| audience_admits(&audience_of(row), present))
        .collect();
    let (chosen, next) = rank_primed(rows, query, limit, priming, breadth);
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
    Ok((chosen.into_iter().map(MemoryRecord::from).collect(), next))
}

#[cfg(test)]
fn rank(
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
) -> Vec<agent_memories::Model> {
    rank_primed(rows, query, limit, &Priming::default(), 1.0).0
}

fn rank_primed(
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> (Vec<agent_memories::Model>, Priming) {
    // Blank and repeated legacy rows must not spend the recall budget.
    let mut seen = std::collections::HashSet::new();
    let rows: Vec<agent_memories::Model> = rows
        .into_iter()
        .filter(|row| {
            let content = normalize_content(&row.content);
            !content.is_empty() && seen.insert(content)
        })
        .collect();
    let concepts: Vec<Vec<Concept>> = rows.iter().map(concepts_of).collect();
    let documents: Vec<super::lexical::Document> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| super::lexical::Document {
            text: &row.content,
            concepts,
        })
        .collect();
    let scores = super::lexical::score_all(query.unwrap_or(""), &documents);
    let strongest = scores
        .iter()
        .filter(|score| score.strong)
        .map(|score| score.value)
        .fold(0.0, f64::max);
    let named: Vec<f64> = scores
        .iter()
        .map(|score| {
            if score.strong {
                score.value / strongest
            } else {
                0.0
            }
        })
        .collect();
    let residual: Vec<f64> = rows.iter().map(|row| priming.of(&row.id)).collect();
    // By weak evidence, then recency (stable: newest-first from the query).
    let mut by_recency: Vec<(f64, usize)> = scores
        .iter()
        .enumerate()
        .map(|(index, score)| (score.value, index))
        .collect();
    by_recency.sort_by(|left, right| right.0.total_cmp(&left.0));
    if named.iter().chain(&residual).all(|seed| *seed <= 0.0) {
        // Nothing named and nothing on the mind: no association starts from
        // a guess.
        let mut rows: Vec<Option<agent_memories::Model>> = rows.into_iter().map(Some).collect();
        let chosen = by_recency
            .into_iter()
            .take(limit)
            .filter_map(|(_, index)| rows[index].take())
            .collect();
        return (chosen, Priming::default());
    }
    let seeds: Vec<f64> = named
        .iter()
        .zip(&residual)
        .map(|(named, residual)| f64::max(*named, *residual))
        .collect();
    let nodes: Vec<super::association::Node> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| super::association::Node {
            concepts,
            at: row.created_at,
        })
        .collect();
    let activation = super::association::spread(&seeds, &nodes);
    let next = next_priming(&rows, &activation);
    let mut scored: Vec<(f64, bool, usize)> = named
        .iter()
        .zip(&activation)
        .enumerate()
        .filter(|(_, (named, activation))| **named > 0.0 || **activation >= ASSOCIATED_MIN)
        .map(|(index, (named, activation))| {
            (
                DIRECT_WEIGHT * named + (1.0 - DIRECT_WEIGHT) * activation,
                *named > 0.0,
                index,
            )
        })
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0));
    // What the query named comes first in number; association fills in, up
    // to half the budget when thought is free to wander.
    let wander = breadth.clamp(0.0, 1.0);
    let mut associated_left = if wander > 0.0 {
        ((limit as f64 * 0.5 * wander).floor() as usize).max(1)
    } else {
        0
    };
    let mut order: Vec<usize> = scored
        .into_iter()
        .filter(|(_, direct, _)| {
            *direct
                || (associated_left > 0 && {
                    associated_left -= 1;
                    true
                })
        })
        .map(|(_, _, index)| index)
        .take(limit)
        .collect();
    if strongest <= 0.0 {
        // Only the lingering topic came to mind: the rest of the budget is
        // the ordinary recent context, as when nothing is on the mind.
        for (_, index) in by_recency {
            if order.len() == limit {
                break;
            }
            if !order.contains(&index) {
                order.push(index);
            }
        }
    }
    let mut rows: Vec<Option<agent_memories::Model>> = rows.into_iter().map(Some).collect();
    let chosen = order
        .into_iter()
        .filter_map(|index| rows[index].take())
        .collect();
    (chosen, next)
}

/// What stays on the mind for the next turn: the most active memories, each
/// fading by half per turn and dropped once it no longer clears the bar.
fn next_priming(rows: &[agent_memories::Model], activation: &[f64]) -> Priming {
    let mut active: Vec<(f64, &str)> = activation
        .iter()
        .zip(rows)
        .map(|(activation, row)| (activation * PRIMING_FADE, row.id.as_str()))
        .filter(|(activation, _)| *activation >= ASSOCIATED_MIN)
        .collect();
    active.sort_by(|left, right| right.0.total_cmp(&left.0));
    Priming {
        by_id: active
            .into_iter()
            .take(PRIMING_KEPT)
            .map(|(activation, id)| (id.to_string(), activation))
            .collect(),
    }
}

/// A person's active memories, newest first, for the memory panel.
pub async fn list<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    limit: u64,
) -> Result<Vec<MemoryRecord>, DbErr> {
    Ok(agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(MemoryRecord::from)
        .collect())
}

/// Edit one active memory of the person. Returns whether a row changed.
pub async fn update_content<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    id: &str,
    content: &str,
) -> Result<bool, DbErr> {
    let content = normalize_content(content);
    if content.is_empty() {
        return Ok(false);
    }
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            content: Set(content),
            updated_at: Set(Utc::now().fixed_offset()),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected == 1)
}

/// One row carried over from a pre-unified store, keeping its identity and
/// history. An id seen before is skipped, so importing twice is harmless.
pub struct ImportedMemory {
    pub id: String,
    pub user_id: i32,
    pub kind: MemoryKind,
    pub content: String,
    pub importance: f64,
    pub access_count: i32,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    pub last_accessed_at: Option<chrono::DateTime<chrono::FixedOffset>>,
}

/// Returns whether the row was new.
pub async fn import<C: ConnectionTrait>(db: &C, memory: ImportedMemory) -> Result<bool, DbErr> {
    let content = normalize_content(&memory.content);
    if memory.user_id <= 0 || content.is_empty() {
        return Ok(false);
    }
    let row = agent_memories::ActiveModel {
        id: Set(memory.id),
        user_id: Set(Some(memory.user_id)),
        kind: Set(memory.kind.as_str().into()),
        content: Set(content),
        evidence: Set(None),
        speaker: Set(Speaker::Import.as_str().into()),
        source: Set("import".into()),
        venue: Set("private".into()),
        audience: Set(json!([memory.user_id])),
        concepts: Set(json!([])),
        importance: Set(memory.importance.clamp(0.0, 1.0)),
        access_count: Set(std::cmp::Ord::max(memory.access_count, 0)),
        last_accessed_at: Set(memory.last_accessed_at),
        valid_from: Set(memory.created_at),
        invalid_at: Set(None),
        invalid_reason: Set(None),
        created_at: Set(memory.created_at),
        updated_at: Set(memory.created_at),
    };
    let inserted = agent_memories::Entity::insert(row)
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(agent_memories::Column::Id)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(inserted > 0)
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
            concepts: json!([]),
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

    const DAY: i64 = 86_400;

    #[test]
    fn matching_rows_rank_first_and_ties_keep_recency() {
        let rows = vec![
            row("new", "likes jasmine tea", 0.5, 0),
            row("mid", "works night shifts", 0.5, 10 * DAY),
            row("old", "prefers saffron tea", 0.5, 20 * DAY),
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
        // A day apart each, so nothing is linked by having been learned together.
        let rows = facts
            .iter()
            .enumerate()
            .map(|(age, text)| row(&age.to_string(), text, 0.5, age as i64 * DAY))
            .collect();
        rank(rows, query, limit)
            .into_iter()
            .map(|row| row.content)
            .collect()
    }

    /// Chinese matches by bigram; repeating a word is not extra evidence;
    /// punctuation is not evidence; nothing overlapping keeps recency.
    #[test]
    fn overlap_is_counted_by_distinct_words_and_bigrams() {
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
        assert_eq!(
            ranked(&["天气好就去跑步", "今天要加班"], Some("今天几点下班"), 2),
            vec!["今天要加班"],
            "sharing 天 alone is not a match"
        );
    }

    fn about(mut row: agent_memories::Model, concepts: &[&str]) -> agent_memories::Model {
        row.concepts = json!(
            concepts
                .iter()
                .map(|name| Concept {
                    name: name.to_string(),
                    aliases: Vec::new(),
                })
                .collect::<Vec<_>>()
        );
        row
    }

    #[test]
    fn what_is_named_brings_its_associations_along() {
        let rows = vec![
            about(row("cat", "养了一只猫叫年糕", 0.5, 0), &["猫", "年糕"]),
            about(row("vet", "年糕上周打了疫苗", 0.5, 30 * DAY), &["年糕"]),
            row("same-chat", "那天刚搬完家", 0.5, 60 + 40 * DAY),
            about(row("tea", "喜欢茉莉花茶", 0.5, 40 * DAY), &["茶"]),
            row("shift", "上夜班", 0.5, 50 * DAY),
        ];
        let ids = |query: &str, limit: usize| -> Vec<String> {
            rank(rows.clone(), Some(query), limit)
                .into_iter()
                .map(|row| row.id)
                .collect()
        };
        assert_eq!(
            ids("猫最近怎么样", 8),
            vec!["cat", "vet"],
            "年糕 links the vaccine to the cat"
        );
        assert_eq!(
            ids("茉莉花茶", 8),
            vec!["tea", "same-chat"],
            "learned a minute apart"
        );
        assert_eq!(ids("猫最近怎么样", 1), vec!["cat"]);
        assert_eq!(
            ids("明日预报", 2),
            vec!["cat", "vet"],
            "nothing named: recency, no association"
        );
    }

    #[test]
    fn a_topic_carries_over_a_turn_that_does_not_name_it_then_fades() {
        let rows = vec![
            row("news", "最近在学吉他", 0.5, 0),
            about(
                row("cat", "养了一只猫叫年糕", 0.5, 10 * DAY),
                &["猫", "年糕"],
            ),
            about(row("vet", "年糕上周打了疫苗", 0.5, 20 * DAY), &["年糕"]),
            row("tea", "喜欢茉莉花茶", 0.5, 30 * DAY),
        ];
        let ids = |chosen: Vec<agent_memories::Model>| -> Vec<String> {
            chosen.into_iter().map(|row| row.id).collect()
        };
        let (first, primed) =
            rank_primed(rows.clone(), Some("猫怎么样"), 3, &Priming::default(), 1.0);
        assert_eq!(ids(first), vec!["cat", "vet"]);
        assert!(primed.of("cat") > 0.0);

        // "它又吐了" names nothing, yet the cat is still on the mind; the
        // rest of the budget is ordinary recent context.
        let (second, primed) = rank_primed(rows.clone(), Some("它又吐了"), 3, &primed, 1.0);
        let second = ids(second);
        assert_eq!(second[0], "cat");
        assert!(second.contains(&"news".to_string()));
        assert_eq!(second.len(), 3);

        // Without the talk renewing it, it is gone within a few turns.
        let mut primed = primed;
        for _ in 0..3 {
            primed = rank_primed(rows.clone(), Some("它又吐了"), 3, &primed, 1.0).1;
        }
        assert!(primed.is_empty(), "{primed:?}");
        let (cold, _) = rank_primed(rows, Some("明日预报"), 2, &primed, 1.0);
        assert_eq!(ids(cold), vec!["news", "cat"], "back to recency");
    }

    #[test]
    fn a_primed_memory_no_longer_admitted_cannot_seed() {
        let rows = vec![row("left", "喜欢茉莉花茶", 0.5, 0)];
        let (chosen, next) =
            rank_primed(rows, Some("它又吐了"), 3, &Priming::with("gone", 1.0), 1.0);
        assert_eq!(chosen.len(), 1, "recency");
        assert!(next.is_empty());
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
            concepts: Vec::new(),
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
        // Learned moments apart, the night shifts come along by association;
        // user 8's identical fact is never in the graph at all.
        assert_eq!(
            tea.iter()
                .map(|memory| memory.content.as_str())
                .collect::<Vec<_>>(),
            vec!["prefers saffron tea", "works night shifts"]
        );
        assert!(tea.iter().all(|memory| memory.user_id == Some(7)));

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

    #[tokio::test]
    async fn importing_the_same_legacy_row_twice_writes_it_once() {
        let Some(db) = temp_db().await else {
            return;
        };
        let legacy = || ImportedMemory {
            id: "json_abc".into(),
            user_id: 7,
            kind: MemoryKind::Lesson,
            content: "动漫角色图 category=anime 效果好".into(),
            importance: 0.7,
            access_count: 2,
            created_at: (Utc::now() - chrono::Duration::days(30)).fixed_offset(),
            last_accessed_at: None,
        };
        assert!(import(&db, legacy()).await.unwrap());
        assert!(!import(&db, legacy()).await.unwrap());
        let rows = active(&db, 7, &[MemoryKind::Lesson]).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].access_count, 2);
        assert_eq!(rows[0].source, "import");
    }
}
