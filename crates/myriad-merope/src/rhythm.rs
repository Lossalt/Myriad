//! How often two people talk, as anyone in a friendship knows it.
//!
//! Three days without a word means nothing between people who write once a
//! month and a great deal between people who write every day. Nobody counts
//! it in days alone: they know how often the two of them usually talk, notice
//! when this time has been longer than that, and notice when, over weeks,
//! they have come to talk more or less than they did. A friendship cools
//! when it stops; it is not measured.
//!
//! So these are facts, from the days they talked: how far apart those days
//! usually are, how long it was this time, and the last four weeks beside the
//! four before. No closeness, no score, nothing she is told to feel: what it
//! means between them is hers.

use chrono::NaiveDate;

/// The latest gaps between their talk days weighed for what is usual.
const GAPS_WEIGHED: usize = 20;
/// Gaps needed before anything is usual: before that they are still getting
/// to know each other's ways.
const USUAL_AFTER: usize = 5;
/// Each of the two stretches set side by side, in days.
const STRETCH: i64 = 28;

/// How often they talk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rhythm {
    /// Days from one of their talk days to the next, as it usually is.
    pub usual_gap: i64,
    /// Days since the last day they talked before today.
    pub since: i64,
    /// Days they talked in the last four weeks and in the four weeks
    /// before, once they have known each other that long.
    pub lately: Option<(usize, usize)>,
}

/// How often they talk, from the days they talked (any order, today
/// possibly among them) and the day they first did; none until there is
/// anything usual.
pub fn rhythm(days: &[NaiveDate], first: NaiveDate, today: NaiveDate) -> Option<Rhythm> {
    let mut past: Vec<NaiveDate> = days.iter().copied().filter(|day| *day < today).collect();
    past.sort_unstable_by(|left, right| right.cmp(left));
    past.dedup();
    let mut gaps: Vec<i64> = past
        .windows(2)
        .take(GAPS_WEIGHED)
        .map(|pair| (pair[0] - pair[1]).num_days())
        .collect();
    if gaps.len() < USUAL_AFTER {
        return None;
    }
    gaps.sort_unstable();
    let usual_gap = gaps[gaps.len() / 2];
    let since = (today - past[0]).num_days();
    let within = |from: i64, to: i64| {
        let mut all: Vec<NaiveDate> = days.to_vec();
        all.sort_unstable();
        all.dedup();
        all.iter()
            .filter(|day| {
                let ago = (today - **day).num_days();
                (from..to).contains(&ago)
            })
            .count()
    };
    let lately = ((today - first).num_days() >= 2 * STRETCH)
        .then(|| (within(0, STRETCH), within(STRETCH, 2 * STRETCH)));
    Some(Rhythm {
        usual_gap,
        since,
        lately,
    })
}

impl Rhythm {
    /// How often they usually talk, in the words people use for it.
    pub fn usually(&self) -> &'static str {
        match self.usual_gap {
            ..=1 => "on most days",
            2..=4 => "every few days",
            5..=10 => "about once a week",
            11..=20 => "every couple of weeks",
            _ => "about once a month or less",
        }
    }

    /// This time has been well past how long it usually is between them.
    pub fn longer_this_time(&self) -> bool {
        self.since >= 2 && self.since >= 2 * self.usual_gap
    }

    /// The last four weeks are clearly unlike the four before: twice as
    /// many talk days or half as many, and more than a few days apart.
    pub fn changed_lately(&self) -> Option<(usize, usize)> {
        self.lately.filter(|(now, before)| {
            let (more, fewer) = ((*now).max(*before), (*now).min(*before));
            more >= 2 * fewer && more - fewer >= 4
        })
    }

    /// What she would have in mind of it while they talk: only when this
    /// time has been longer than usual or lately is unlike before, and then
    /// how it usually is beside it.
    pub fn while_talking(&self) -> Option<String> {
        let longer = self.longer_this_time();
        let changed = self.changed_lately();
        if !longer && changed.is_none() {
            return None;
        }
        let mut line = format!("You two usually talk {}.", self.usually());
        if longer {
            line.push_str(&format!(
                " Before today, the last day they wrote was {} days ago.",
                self.since
            ));
        }
        if let Some((now, before)) = changed {
            line.push_str(&format!(
                " They wrote on {now} days in the last four weeks, against {before} in the four weeks before."
            ));
        }
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: i64) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 14).unwrap() - chrono::Duration::days(n)
    }

    /// Days ago, newest first, from `ago`.
    fn days(ago: &[i64]) -> Vec<NaiveDate> {
        ago.iter().map(|n| day(*n)).collect()
    }

    #[test]
    fn nothing_is_usual_while_they_are_still_getting_to_know_each_other() {
        assert_eq!(rhythm(&days(&[0, 1, 2, 3, 4]), day(4), day(0)), None);
        assert!(rhythm(&days(&[1, 2, 3, 4, 5, 6]), day(6), day(0)).is_some());
    }

    /// Three days apart is nothing for once-a-week friends and a long
    /// while for everyday ones.
    #[test]
    fn longer_than_usual_is_theirs_not_a_number_of_days() {
        let daily: Vec<i64> = (3..30).collect();
        let daily = rhythm(&days(&daily), day(29), day(0)).unwrap();
        assert_eq!((daily.usual_gap, daily.since), (1, 3));
        assert_eq!(daily.usually(), "on most days");
        assert!(daily.longer_this_time());
        let weekly = rhythm(&days(&[3, 10, 17, 24, 31, 38, 45]), day(45), day(0)).unwrap();
        assert_eq!(weekly.usual_gap, 7);
        assert_eq!(weekly.usually(), "about once a week");
        assert!(!weekly.longer_this_time());
        assert_eq!(weekly.while_talking(), None, "nothing to say");
        // Talking today does not make it short: the gap is before today.
        let today_too = rhythm(&days(&[0, 3, 4, 5, 6, 7, 8, 9]), day(9), day(0)).unwrap();
        assert_eq!(today_too.since, 3);
    }

    /// Every day for weeks, then a handful: the last four weeks beside the
    /// four before say so; a few days either way do not.
    #[test]
    fn lately_unlike_before_is_said_with_how_it_usually_is() {
        let mut ago: Vec<i64> = vec![1, 6, 13, 20];
        ago.extend(28..56);
        let cooling = rhythm(&days(&ago), day(60), day(0)).unwrap();
        assert_eq!(cooling.lately, Some((4, 28)));
        assert_eq!(cooling.changed_lately(), Some((4, 28)));
        let line = cooling.while_talking().unwrap();
        assert!(line.contains("usually talk on most days"), "{line}");
        assert!(
            line.contains("4 days in the last four weeks, against 28"),
            "{line}"
        );
        let steady: Vec<i64> = (1..56).step_by(2).collect();
        let steady = rhythm(&days(&steady), day(60), day(0)).unwrap();
        assert_eq!(steady.changed_lately(), None);
        // Known for less than eight weeks: nothing to set side by side.
        let new: Vec<i64> = (1..30).collect();
        assert_eq!(rhythm(&days(&new), day(29), day(0)).unwrap().lately, None);
    }
}
