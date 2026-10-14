//! Chance, as the end hands it in: the numbers it gives first, then a fixed
//! generator from its seed, so the same step with the same seed rolls the
//! same everywhere.

use serde::{Deserialize, Serialize};

/// Uniform numbers in [0, 1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rolls {
    given: std::collections::VecDeque<f64>,
    state: u64,
}

impl Rolls {
    /// `given` first (each clamped into [0, 1)), then from `seed`.
    pub fn new(seed: u64, given: Vec<f64>) -> Self {
        Self {
            given: given
                .into_iter()
                .map(|roll| roll.clamp(0.0, 1.0 - f64::EPSILON))
                .collect(),
            state: seed,
        }
    }

    pub fn roll(&mut self) -> f64 {
        if let Some(roll) = self.given.pop_front() {
            return roll;
        }
        // SplitMix64.
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A whole number in `range`, uniform.
    pub fn range(&mut self, range: std::ops::RangeInclusive<u64>) -> u64 {
        let span = range.end() - range.start() + 1;
        range.start() + ((self.roll() * span as f64) as u64).min(span - 1)
    }
}
