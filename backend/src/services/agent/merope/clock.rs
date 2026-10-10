//! What "now" is while her words are put together: the clock, except where
//! a turn is put together as of another moment (a simulated day, a real
//! turn answered again), so that the hour, how long ago things were, and
//! what has come due are as they were then. Scoped to the one task putting
//! the prompt together; nothing else sees it.
//!
//! And the zone her day runs in: the one her persona names (`zone`), or the
//! site's own when it names none.

use std::{fmt, str::FromStr, sync::RwLock};

use chrono::{
    DateTime, FixedOffset, Local, MappedLocalTime, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc,
};

tokio::task_local! {
    static AT: DateTime<Utc>;
}

/// Now, as this turn is put together.
pub fn now() -> DateTime<Utc> {
    AT.try_with(|at| *at).unwrap_or_else(|_| Utc::now())
}

/// The zone her day runs in: the one her persona names, or the site's own,
/// which is the process's (`TZ`). The rules in `myriad_merope` take it as an
/// input and never read the process zone themselves.
pub fn zone() -> Zone {
    ZONE.read().map(|zone| *zone).unwrap_or(Zone::Site)
}

/// [`now`], in her zone.
pub fn local_now() -> DateTime<Zone> {
    now().with_timezone(&zone())
}

/// The zone her persona row names, as it was last read or written: every
/// read of her persona keeps this current, so a change made through another
/// replica arrives with the next read. Empty, unknown, or no persona at all
/// is the site's own zone.
pub fn note_zone(named: Option<&str>) {
    let zone = named.and_then(Zone::named).unwrap_or(Zone::Site);
    if let Ok(mut held) = ZONE.write() {
        *held = zone;
    }
}

static ZONE: RwLock<Zone> = RwLock::new(Zone::Site);

/// Her zone: an IANA zone, or the site's own (the process's `Local`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Named(chrono_tz::Tz),
    Site,
}

impl Zone {
    /// An IANA name (`Asia/Shanghai`), trimmed; none when empty or unknown.
    pub fn named(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        chrono_tz::Tz::from_str(raw).ok().map(Self::Named)
    }

    /// Whether `raw` is a zone her persona may name: empty (the site's own)
    /// or a known IANA name.
    pub fn is_valid_name(raw: &str) -> bool {
        raw.trim().is_empty() || Self::named(raw).is_some()
    }
}

/// An offset in her zone, carrying the zone it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneOffset {
    zone: Zone,
    fixed: FixedOffset,
}

impl Offset for ZoneOffset {
    fn fix(&self) -> FixedOffset {
        self.fixed
    }
}

impl fmt::Display for ZoneOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.fixed, f)
    }
}

impl Zone {
    fn offset(self, fixed: FixedOffset) -> ZoneOffset {
        ZoneOffset { zone: self, fixed }
    }
}

impl TimeZone for Zone {
    type Offset = ZoneOffset;

    fn from_offset(offset: &ZoneOffset) -> Self {
        offset.zone
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<ZoneOffset> {
        match self {
            Zone::Named(tz) => tz
                .offset_from_local_date(local)
                .map(|offset| self.offset(offset.fix())),
            Zone::Site => Local
                .offset_from_local_date(local)
                .map(|offset| self.offset(offset)),
        }
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<ZoneOffset> {
        match self {
            Zone::Named(tz) => tz
                .offset_from_local_datetime(local)
                .map(|offset| self.offset(offset.fix())),
            Zone::Site => Local
                .offset_from_local_datetime(local)
                .map(|offset| self.offset(offset)),
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> ZoneOffset {
        match self {
            Zone::Named(tz) => self.offset(tz.offset_from_utc_date(utc).fix()),
            Zone::Site => self.offset(Local.offset_from_utc_date(utc)),
        }
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> ZoneOffset {
        match self {
            Zone::Named(tz) => self.offset(tz.offset_from_utc_datetime(utc).fix()),
            Zone::Site => self.offset(Local.offset_from_utc_datetime(utc)),
        }
    }
}

/// Put `work` together as of `at`.
#[cfg(test)]
pub(crate) async fn as_of<F: std::future::Future>(at: DateTime<Utc>, work: F) -> F::Output {
    AT.scope(at, work).await
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, Timelike};

    use super::*;

    #[tokio::test]
    async fn a_turn_put_together_as_of_then_sees_then_and_nothing_else_does() {
        let then = chrono::Utc::now() - chrono::Duration::days(3);
        let seen = super::as_of(then, async { super::now() }).await;
        assert_eq!(seen, then);
        assert!(chrono::Utc::now() - super::now() < chrono::Duration::seconds(5));
    }

    #[test]
    fn her_zone_is_a_named_one_or_the_sites_own() {
        assert_eq!(Zone::named(""), None);
        assert_eq!(Zone::named("  "), None);
        assert_eq!(Zone::named("Mars/Olympus"), None);
        assert!(Zone::is_valid_name(""));
        assert!(Zone::is_valid_name(" Europe/Berlin "));
        assert!(!Zone::is_valid_name("+08:00"));
        let berlin = Zone::named("Europe/Berlin").unwrap();
        // Summer and winter: the named zone's own rules, not a fixed offset.
        let summer = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
        let winter = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(summer.with_timezone(&berlin).hour(), 14);
        assert_eq!(winter.with_timezone(&berlin).hour(), 13);
        // Late at night in Berlin is already tomorrow in Shanghai.
        let late = Utc.with_ymd_and_hms(2026, 7, 1, 21, 30, 0).unwrap();
        let shanghai = Zone::named("Asia/Shanghai").unwrap();
        assert_eq!(late.with_timezone(&berlin).day(), 1);
        assert_eq!(late.with_timezone(&shanghai).day(), 2);
        // The site's own is the process zone, to the second.
        let here = late.with_timezone(&Zone::Site);
        assert_eq!(here.naive_local(), late.with_timezone(&Local).naive_local());
        // A local midnight in the zone maps back to the same instant.
        let midnight = NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let start = berlin.from_local_datetime(&midnight).earliest().unwrap();
        assert_eq!(start.naive_local(), midnight);
        assert_eq!(start.timezone(), berlin);
    }
}
