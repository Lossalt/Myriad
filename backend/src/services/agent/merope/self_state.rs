//! The persona's own state, not toward anyone: how much energy she has and
//! how much she wants company.
//!
//! Derived, never stored. It is read off facts that already exist — the hour
//! of her day and when each person last talked to her — so it cannot drift
//! from what actually happened, survives restarts, and needs no reset.
//!
//! - **Energy** follows the day (low at night) and drops with how many
//!   different people she has been talking to lately. Five people wear her out
//!   more than five turns with one.
//! - **Social** is the want for company: it grows with the time since anyone
//!   last talked to her and is gone while someone is.
//! - **Curiosity** is the want to know: it grows with the time since she last
//!   learned anything new about anyone, and learning eases it.
//!
//! It turns real knobs: how long she waits between speaking up unprompted,
//! how far recall wanders by association, how often her mind drifts and
//! whether she asks, and a line in the speaking prompt.
//! Numbers never reach a prompt.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Timelike, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use crate::models::entities::agent_addressee_state;

/// Energy each recent person costs at the moment they spoke.
const FATIGUE_PER_PERSON: f64 = 12.0;
/// A conversation's weight on energy falls by e every this many hours.
const FATIGUE_TAU_H: f64 = 1.5;
/// The want for company approaches its peak over a few hours of silence.
const SOCIAL_TAU_H: f64 = 3.0;
/// Only this far back counts toward either.
const LOOKBACK_H: i64 = 24;
/// The want to know builds over most of a day without anything new.
const CURIOSITY_TAU_H: f64 = 6.0;
const CACHE_FOR: Duration = Duration::from_secs(30);
/// The shortest wait between unprompted words; tiredness only lengthens it.
pub const BASE_PROACTIVE_COOLDOWN_SECS: i64 = 180;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfState {
    /// 0–100.
    pub energy: f64,
    /// 0–100: 0 is company right now, 100 a long silence.
    pub social: f64,
    /// 0–100: 0 just learned something, 100 nothing new for long.
    pub curiosity: f64,
}

impl SelfState {
    pub fn tired(&self) -> bool {
        self.energy < 40.0
    }

    pub fn lively(&self) -> bool {
        self.energy >= 65.0
    }

    pub fn lonely(&self) -> bool {
        self.social >= 70.0
    }

    pub fn curious(&self) -> bool {
        self.curiosity >= 60.0
    }

    /// The same state, with curiosity from how long ago she last learned
    /// something new (`None`: never).
    pub fn with_last_learned(self, hours_ago: Option<f64>) -> Self {
        let hours = hours_ago
            .filter(|hours| hours.is_finite() && *hours >= 0.0)
            .unwrap_or(f64::INFINITY);
        Self {
            curiosity: 100.0 * (1.0 - (-hours / CURIOSITY_TAU_H).exp()),
            ..self
        }
    }

    /// How long to wait after speaking up before speaking up again. Never
    /// shorter than the base; a tired persona leaves people alone longer.
    pub fn proactive_cooldown_secs(&self) -> i64 {
        let spent = 1.0 - self.energy.clamp(0.0, 100.0) / 100.0;
        (BASE_PROACTIVE_COOLDOWN_SECS as f64 * (1.0 + 3.0 * spent * spent)).round() as i64
    }

    /// How far recall may wander by association, `0..=1`. Tiredness narrows
    /// thought to what was actually said.
    pub fn recall_breadth(&self) -> f64 {
        if self.tired() { 0.5 } else { 1.0 }
    }

    /// Coarse bands for a decision model, which never sees the numbers.
    pub fn bands(&self) -> crate::services::agent::consciousness::SelfBands {
        let energy = if self.tired() {
            "low"
        } else if self.lively() {
            "high"
        } else {
            "normal"
        };
        let company = if self.lonely() { "wanted" } else { "content" };
        let curiosity = if self.curious() { "high" } else { "normal" };
        crate::services::agent::consciousness::SelfBands {
            energy: energy.into(),
            company: company.into(),
            curiosity: curiosity.into(),
        }
    }
}

/// Energy at this hour of her day, before anyone has tired her.
fn day_energy(hour: u32) -> f64 {
    match hour {
        0..=5 => 25.0,
        6 => 40.0,
        7..=8 => 55.0,
        9..=17 => 75.0,
        18..=21 => 65.0,
        22 => 50.0,
        _ => 35.0,
    }
}

/// `contacts` are, for each person, hours since they last talked to her.
pub fn derive(hour: u32, contacts: &[f64]) -> SelfState {
    let load: f64 = contacts
        .iter()
        .filter(|hours| hours.is_finite() && **hours >= 0.0)
        .map(|hours| (-hours / FATIGUE_TAU_H).exp())
        .sum();
    let energy = (day_energy(hour) - FATIGUE_PER_PERSON * load).clamp(5.0, 100.0);
    let silence = contacts
        .iter()
        .copied()
        .filter(|hours| hours.is_finite() && *hours >= 0.0)
        .fold(LOOKBACK_H as f64, f64::min);
    let social = 100.0 * (1.0 - (-silence / SOCIAL_TAU_H).exp());
    SelfState {
        energy,
        social,
        curiosity: 0.0,
    }
}

static CACHE: LazyLock<Mutex<Option<(Instant, SelfState)>>> = LazyLock::new(|| Mutex::new(None));

/// Her state now. Recomputed at most every half minute.
pub async fn current(db: &DatabaseConnection) -> SelfState {
    if let Some(state) = CACHE
        .lock()
        .ok()
        .and_then(|cached| *cached)
        .filter(|(at, _)| at.elapsed() < CACHE_FOR)
        .map(|(_, state)| state)
    {
        return state;
    }
    let now = Utc::now();
    let contacts = recent_contacts(db, now).await.unwrap_or_default();
    let learned = crate::services::agent::memory::unified::last_learned_at(db)
        .await
        .ok()
        .flatten()
        .map(|at| (now - at.with_timezone(&Utc)).num_seconds().max(0) as f64 / 3600.0);
    // Her day runs on the host clock, as the do-not-disturb window does.
    let state = derive(chrono::Local::now().hour(), &contacts).with_last_learned(learned);
    if let Ok(mut cached) = CACHE.lock() {
        *cached = Some((Instant::now(), state));
    }
    state
}

async fn recent_contacts(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
) -> Result<Vec<f64>, sea_orm::DbErr> {
    let since = (now - chrono::Duration::hours(LOOKBACK_H)).fixed_offset();
    let spoken: Vec<Option<chrono::DateTime<chrono::FixedOffset>>> =
        agent_addressee_state::Entity::find()
            .select_only()
            .column(agent_addressee_state::Column::LastUserMessageAt)
            .filter(agent_addressee_state::Column::LastUserMessageAt.gt(since))
            .into_tuple()
            .all(db)
            .await?;
    Ok(spoken
        .into_iter()
        .flatten()
        .map(|at| (now - at.with_timezone(&Utc)).num_seconds().max(0) as f64 / 3600.0)
        .collect())
}

/// A line about herself for the speaking prompt, when there is one worth
/// saying. Generic for every persona: tiredness is not coldness, and wanting
/// company is not a claim on this person.
pub fn format_self_section(state: &SelfState) -> Option<String> {
    let line = if state.tired() {
        "You are low on energy right now: shorter replies, fewer tangents. It is tiredness, not coldness toward them."
    } else if state.lively() && state.lonely() {
        "Nobody has talked with you for a while and you have energy: you are glad of the company and may say a little more."
    } else {
        return None;
    };
    Some(format!("## Yourself\n{line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_night_and_many_people_tire_her() {
        assert!(derive(14, &[]).lively());
        assert!(derive(3, &[]).tired(), "night");
        let one = derive(14, &[0.1]);
        assert!(!one.tired());
        let crowd = derive(14, &[0.1, 0.2, 0.2, 0.3, 0.5]);
        assert!(crowd.tired(), "{crowd:?}");
        let long_ago = derive(14, &[10.0, 11.0, 12.0, 13.0, 14.0]);
        assert!(long_ago.lively(), "rested since: {long_ago:?}");
    }

    #[test]
    fn silence_makes_her_want_company_and_talk_ends_it() {
        assert!(derive(14, &[]).lonely(), "no one all day");
        assert!(derive(14, &[5.0]).lonely());
        assert!(!derive(14, &[0.05]).lonely());
        assert!(derive(14, &[0.05]).social < 5.0);
    }

    #[test]
    fn tiredness_only_lengthens_the_wait_and_narrows_recall() {
        let fresh = SelfState {
            energy: 100.0,
            social: 50.0,
            curiosity: 0.0,
        };
        let spent = SelfState {
            energy: 5.0,
            social: 50.0,
            curiosity: 0.0,
        };
        assert_eq!(
            fresh.proactive_cooldown_secs(),
            BASE_PROACTIVE_COOLDOWN_SECS
        );
        assert!(spent.proactive_cooldown_secs() > 3 * BASE_PROACTIVE_COOLDOWN_SECS);
        assert_eq!(fresh.recall_breadth(), 1.0);
        assert!(spent.recall_breadth() < 1.0);
    }

    #[test]
    fn nothing_new_for_long_makes_her_curious_and_learning_eases_it() {
        let base = derive(14, &[]);
        assert!(
            base.with_last_learned(None).curious(),
            "never learned anything"
        );
        assert!(base.with_last_learned(Some(10.0)).curious());
        assert!(!base.with_last_learned(Some(0.5)).curious());
        assert_eq!(
            base.with_last_learned(Some(0.5)).bands().curiosity,
            "normal"
        );
        assert_eq!(base.with_last_learned(Some(12.0)).bands().curiosity, "high");
    }

    #[test]
    fn the_self_line_is_generic_and_numberless() {
        let tired = format_self_section(&derive(3, &[])).unwrap();
        assert!(tired.contains("not coldness"));
        let lonely = format_self_section(&derive(14, &[])).unwrap();
        assert!(lonely.contains("glad of the company"));
        assert!(format_self_section(&derive(14, &[0.1])).is_none());
        for section in [tired, lonely] {
            assert!(!section.chars().any(|ch| ch.is_ascii_digit()));
        }
    }
}
