//! When a reminder is due: the one rule every end runs, so a reminder the
//! site keeps and one a Mac keeps come due at the same moment.
//!
//! `reminder.create` stores `datetime` as the model wrote it and `repeat`
//! (`none`, `daily`, `weekly`, `monthly`). The schema asks for RFC 3339, but
//! nothing enforces the format, so `datetime` is read leniently: RFC 3339
//! first, then the same with `:00` seconds put in when they are missing; a
//! text with no offset is a wall-clock time in the zone the caller names.
//!
//! A repeating reminder steps from its first time in that zone's calendar:
//! a 9:00 daily stays 9:00 across a DST change, and a monthly one on the
//! 31st falls back to a shorter month's last day and returns to the 31st.
//! Any other `repeat` value is a single reminder. Occurrences missed while
//! nothing was checking fold into the latest one, and nothing is due twice
//! for the same time ([`due`]).

use chrono::{
    DateTime, Datelike, FixedOffset, LocalResult, NaiveDateTime, Offset, TimeDelta, TimeZone, Utc,
};

/// How a reminder repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Never,
    Days(u32),
    Months(u32),
}

impl Repeat {
    /// `reminder.create`'s `repeat`; anything but its three steps never repeats.
    pub fn of(repeat: &str) -> Self {
        match repeat {
            "daily" => Self::Days(1),
            "weekly" => Self::Days(7),
            "monthly" => Self::Months(1),
            _ => Self::Never,
        }
    }
}

/// The reminder's `datetime`, as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// An instant: the text carried its offset.
    At(DateTime<FixedOffset>),
    /// A wall-clock time with no offset, read in the caller's zone.
    Local(NaiveDateTime),
}

/// `YYYY-MM-DDTHH:MM` followed by anything but `:` gets `:00` after the minutes.
fn with_seconds(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    if bytes.len() < 16 || bytes[13] != b':' || (bytes.len() > 16 && bytes[16] == b':') {
        return None;
    }
    if !raw.is_char_boundary(16) {
        return None;
    }
    Some(format!("{}:00{}", &raw[..16], &raw[16..]))
}

/// Reads `datetime`: RFC 3339, then with seconds put in; failing both, each
/// of those as a wall-clock time (no offset).
pub fn parse(raw: &str) -> Option<When> {
    let candidates: Vec<String> = std::iter::once(raw.to_string())
        .chain(with_seconds(raw))
        .collect();
    for text in &candidates {
        if let Ok(at) = DateTime::parse_from_rfc3339(text) {
            return Some(When::At(at));
        }
    }
    for text in &candidates {
        if let Ok(at) = DateTime::parse_from_rfc3339(&format!("{text}Z")) {
            return Some(When::Local(at.naive_utc()));
        }
    }
    None
}

/// The instant a wall-clock time names in `zone`: the earlier one when the
/// clocks go back, and one moved forward past the gap when they skip it.
fn resolve<Tz: TimeZone>(local: NaiveDateTime, zone: &Tz) -> Option<DateTime<Utc>> {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(at) => Some(at.with_timezone(&Utc)),
        LocalResult::Ambiguous(earlier, _) => Some(earlier.with_timezone(&Utc)),
        LocalResult::None => {
            // Read with the offset in force before the gap, it lands as far
            // past the gap as it was into it.
            let before = zone
                .offset_from_utc_datetime(&(local - TimeDelta::days(1)))
                .fix();
            Some(Utc.from_utc_datetime(
                &(local - TimeDelta::seconds(i64::from(before.local_minus_utc()))),
            ))
        }
    }
}

impl When {
    /// The instant this names; a wall-clock time in `zone`.
    pub fn instant<Tz: TimeZone>(&self, zone: &Tz) -> Option<DateTime<Utc>> {
        match self {
            Self::At(at) => Some(at.with_timezone(&Utc)),
            Self::Local(local) => resolve(*local, zone),
        }
    }
}

/// The `n`th time after `first`, stepped in `zone`'s calendar.
fn occurrence<Tz: TimeZone>(
    first: DateTime<Utc>,
    repeat: Repeat,
    n: u32,
    zone: &Tz,
) -> Option<DateTime<Utc>> {
    let local = first.with_timezone(zone).naive_local();
    let stepped = match repeat {
        Repeat::Never => return (n == 0).then_some(first),
        Repeat::Days(days) => {
            local.checked_add_signed(TimeDelta::days(i64::from(days) * i64::from(n)))?
        }
        Repeat::Months(months) => {
            local.checked_add_months(chrono::Months::new(months.checked_mul(n)?))?
        }
    };
    if n == 0 {
        return Some(first);
    }
    resolve(stepped, zone)
}

/// The latest time the reminder comes at, at or before `now`; none while the
/// first is still ahead, or when `datetime` cannot be read.
pub fn latest<Tz: TimeZone>(
    datetime: &str,
    repeat: &str,
    now: DateTime<Utc>,
    zone: &Tz,
) -> Option<DateTime<Utc>> {
    let first = parse(datetime)?.instant(zone)?;
    if first > now {
        return None;
    }
    let repeat = Repeat::of(repeat);
    let mut n: u32 = match repeat {
        Repeat::Never => return Some(first),
        Repeat::Days(days) => {
            let span = (now - first).num_seconds() / (86_400 * i64::from(days));
            u32::try_from(span.max(0)).unwrap_or(u32::MAX)
        }
        Repeat::Months(months) => {
            let (from, to) = (first.with_timezone(zone), now.with_timezone(zone));
            let span = (to.year() - from.year()) * 12 + to.month() as i32 - from.month() as i32;
            u32::try_from(span.max(0)).unwrap_or(0) / months.max(1)
        }
    };
    while n > 0 && occurrence(first, repeat, n, zone).is_none_or(|at| at > now) {
        n -= 1;
    }
    while let Some(next) = occurrence(first, repeat, n + 1, zone) {
        if next > now {
            break;
        }
        n += 1;
    }
    occurrence(first, repeat, n, zone).or(Some(first))
}

/// What to remind of now: the latest time not yet reminded of
/// (`reminded_through`). Times missed meanwhile fold into one, and the same
/// time never comes twice.
pub fn due<Tz: TimeZone>(
    datetime: &str,
    repeat: &str,
    reminded_through: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    zone: &Tz,
) -> Option<DateTime<Utc>> {
    let at = latest(datetime, repeat, now, zone)?;
    if reminded_through.is_some_and(|through| at <= through) {
        return None;
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Tz;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn datetime_is_read_leniently_and_an_offset_is_never_reinterpreted() {
        let shanghai: Tz = "Asia/Shanghai".parse().unwrap();
        let york: Tz = "America/New_York".parse().unwrap();
        let local = parse("2026-10-09T09:00").unwrap();
        assert!(matches!(local, When::Local(_)));
        assert_eq!(local.instant(&shanghai), Some(at("2026-10-09T01:00:00Z")));
        assert_eq!(local.instant(&york), Some(at("2026-10-09T13:00:00Z")));
        assert_eq!(
            parse("2026-10-09T09:00:00+08:00").unwrap().instant(&york),
            Some(at("2026-10-09T01:00:00Z"))
        );
        assert_eq!(
            parse("2026-10-09 09:00+08:00").unwrap().instant(&york),
            Some(at("2026-10-09T01:00:00Z"))
        );
        // A wall-clock time skipped by DST moves forward past the gap.
        assert_eq!(
            parse("2026-03-08T02:30:00").unwrap().instant(&york),
            Some(at("2026-03-08T07:30:00Z"))
        );
        // Repeated by DST: the earlier one.
        assert_eq!(
            parse("2026-11-01T01:30:00").unwrap().instant(&york),
            Some(at("2026-11-01T05:30:00Z"))
        );
        assert_eq!(parse("明天早上"), None);
        assert_eq!(parse("2026-02-30T09:00:00Z"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn repeats_keep_the_wall_clock_and_the_day_of_month() {
        let york: Tz = "America/New_York".parse().unwrap();
        let utc = Utc;
        // Daily 09:00 New York across the 2026-11-01 DST change stays 09:00 local.
        let daily = "2026-10-30T09:00:00";
        assert_eq!(
            latest(daily, "daily", at("2026-11-02T15:00:00Z"), &york),
            Some(at("2026-11-02T14:00:00Z"))
        );
        assert_eq!(
            latest(daily, "daily", at("2026-10-31T12:59:59Z"), &york),
            Some(at("2026-10-30T13:00:00Z"))
        );
        let weekly = "2026-10-01T09:00:00Z";
        assert_eq!(
            latest(weekly, "weekly", at("2026-10-21T08:59:59Z"), &utc),
            Some(at("2026-10-15T09:00:00Z"))
        );
        assert_eq!(
            latest(weekly, "weekly", at("2026-10-22T09:00:00Z"), &utc),
            Some(at("2026-10-22T09:00:00Z"))
        );
        let monthly = "2026-01-31T09:00:00Z";
        assert_eq!(
            latest(monthly, "monthly", at("2026-02-28T10:00:00Z"), &utc),
            Some(at("2026-02-28T09:00:00Z"))
        );
        assert_eq!(
            latest(monthly, "monthly", at("2026-03-31T09:00:00Z"), &utc),
            Some(at("2026-03-31T09:00:00Z"))
        );
        assert_eq!(
            latest(monthly, "monthly", at("2026-04-29T09:00:00Z"), &utc),
            Some(at("2026-03-31T09:00:00Z"))
        );
        // The enum is the whole list; anything else is a single reminder.
        for value in ["none", "yearly", "Daily", ""] {
            assert_eq!(
                latest(
                    "2026-10-01T09:00:00Z",
                    value,
                    at("2027-01-01T00:00:00Z"),
                    &utc
                ),
                Some(at("2026-10-01T09:00:00Z")),
                "{value}"
            );
        }
        assert_eq!(
            latest(
                "2026-10-01T09:00:00Z",
                "none",
                at("2026-10-01T08:59:59Z"),
                &utc
            ),
            None
        );
        assert_eq!(
            latest("明天早上", "daily", at("2030-01-01T00:00:00Z"), &utc),
            None
        );
    }

    #[test]
    fn missed_times_fold_into_one_and_nothing_comes_twice() {
        let utc = Utc;
        let daily = "2026-10-01T09:00:00Z";
        let first = due(daily, "daily", None, at("2026-10-05T12:00:00Z"), &utc);
        assert_eq!(first, Some(at("2026-10-05T09:00:00Z")));
        assert_eq!(
            due(daily, "daily", first, at("2026-10-05T23:59:00Z"), &utc),
            None
        );
        assert_eq!(
            due(daily, "daily", first, at("2026-10-06T09:00:00Z"), &utc),
            Some(at("2026-10-06T09:00:00Z"))
        );
        let once = "2026-10-01T09:00:00Z";
        let shown = due(once, "none", None, at("2026-11-01T00:00:00Z"), &utc);
        assert_eq!(shown, Some(at("2026-10-01T09:00:00Z")));
        assert_eq!(
            due(once, "none", shown, at("2027-01-01T00:00:00Z"), &utc),
            None
        );
    }
}
