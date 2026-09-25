//! What was on the persona's mind in each conversation a moment ago.
//!
//! Chat turns hand their recall's leftover activation to the next turn of the
//! same conversation (see `memory::unified::Priming`). This is attention, not
//! memory: it lives in process memory only, lapses when the talk goes quiet,
//! and losing it on restart costs one turn of continuity. It holds memory ids,
//! never text, and every recall re-checks those ids against the rows the
//! present audience may hear.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::services::agent::memory::unified::Priming;

/// A pause this long ends the conversation's train of thought.
const IDLE: Duration = Duration::from_secs(30 * 60);
/// Upper bound on conversations held at once; the stalest go first.
const MAX_HELD: usize = 10_000;

static HELD: LazyLock<Mutex<HashMap<i32, (Instant, Priming)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The priming left by this person's last turn, if the talk is still warm.
/// A new persona has nothing on her mind from before.
pub(super) fn forget() {
    if let Ok(mut held) = HELD.lock() {
        held.clear();
    }
}

pub fn current(user_id: i32) -> Priming {
    let Ok(held) = HELD.lock() else {
        return Priming::default();
    };
    held.get(&user_id)
        .filter(|(at, _)| at.elapsed() < IDLE)
        .map(|(_, priming)| priming.clone())
        .unwrap_or_default()
}

/// Keep what this turn left active for the next one.
pub fn keep(user_id: i32, priming: Priming) {
    let Ok(mut held) = HELD.lock() else {
        return;
    };
    if priming.is_empty() {
        held.remove(&user_id);
        return;
    }
    held.retain(|_, (at, _)| at.elapsed() < IDLE);
    if held.len() >= MAX_HELD && !held.contains_key(&user_id) {
        if let Some(stalest) = held
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(id, _)| *id)
        {
            held.remove(&stalest);
        }
    }
    held.insert(user_id, (Instant::now(), priming));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_hands_its_priming_to_the_next_and_an_empty_one_clears_it() {
        let user = -91_001;
        assert!(current(user).is_empty());
        keep(user, Priming::with("mem_cat", 0.5));
        assert_eq!(current(user), Priming::with("mem_cat", 0.5));
        assert!(
            current(user + 1).is_empty(),
            "another person's mind is separate"
        );
        keep(user, Priming::default());
        assert!(current(user).is_empty());
    }
}
