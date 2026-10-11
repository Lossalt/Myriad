//! Her present across people: what happened to her with others since she got
//! up.
//!
//! A real person carries the last hour into the next talk: still glad from a
//! compliment, still smarting from a jab, worn from a busy group. Her talks
//! are kept apart (what one person tells her is theirs), so what crosses
//! between them is only how it went, never who or what about. It crosses
//! two ways:
//! - **How today has left her** ([`own_feeling`]): one state, not toward
//!   anyone, that every push of her emotion moves and time lets fade. It is
//!   part of her own state (energy and the rest), so it reaches whatever she
//!   judges, not only what she says, and narrows her when low.
//! - **Her talks** ([`talks`], [`section`]): told in private, as what she
//!   could say if she chose to, roughly and without who.
//!
//! That holds by what goes in, not by asking the model to keep quiet: every
//! input here is a time, a kind or a number. No line anyone wrote, no name,
//! no group and no topic reaches this module, so nothing it tells can give
//! one away. The caller keeps the people and groups apart (`person`, the
//! group's venue) only to leave out the one she is facing and to group what
//! happened into talks; neither is ever told.

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::sore::Weight;

/// Messages further apart than this belong to different talks.
pub const TALK_GAP: Duration = Duration::minutes(45);
/// How long after their last message a change of her affect still belongs
/// to that talk (her appraisal of their words lands a little later).
const SETTLES_WITHIN: Duration = Duration::minutes(10);
/// Talks that left something, told one by one, newest first; past these,
/// and the talks that left nothing, they are counted together.
pub const TOLD_ONE_BY_ONE: usize = 6;
/// How far her emotion is off even before it is told (as the emotion
/// section reads it).
const CLEAR: f64 = 10.0;
const STRONG: f64 = 25.0;

/// Where a talk happened. Only the kind is ever told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Private,
    /// A group, by its venue (`telegram:-100123`): kept only to tell one
    /// group's talk from another's and to leave out the one she is in.
    Group(String),
    /// Something done for them (Work).
    Work,
}

/// A message someone sent her.
#[derive(Debug, Clone)]
pub struct Heard {
    pub person: i32,
    pub place: Place,
    pub at: DateTime<Utc>,
}

/// Why her affect toward someone moved, as the change was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Turn,
    Praise,
    Scold,
    Appraisal,
    TaskDone,
    TaskFailed,
    Other,
}

impl Cause {
    /// From the recorded cause (`agent_affect_events.cause`).
    pub fn parse(cause: &str) -> Self {
        match cause {
            "user_turn" => Self::Turn,
            "user_praise" => Self::Praise,
            "user_scold" => Self::Scold,
            "user_appraisal" => Self::Appraisal,
            "task_ok" => Self::TaskDone,
            "task_fail" => Self::TaskFailed,
            _ => Self::Other,
        }
    }
}

/// One change of her affect toward someone: why, and her emotion after it,
/// off even (0 is even).
#[derive(Debug, Clone)]
pub struct Moved {
    pub person: i32,
    pub at: DateTime<Utc>,
    pub cause: Cause,
    pub felt_after: f64,
}

/// Something someone did that got to her, from her sore spots: how much,
/// where, when, and when they made it right. What it was stays with them.
#[derive(Debug, Clone)]
pub struct Stung {
    pub person: i32,
    pub place: Place,
    pub weight: Weight,
    pub since: DateTime<Utc>,
    pub mended: Option<DateTime<Utc>>,
}

/// One talk with someone else, as it is told.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Talk {
    pub kind: TalkKind,
    pub started: DateTime<Utc>,
    pub ended: DateTime<Utc>,
    #[serde(default)]
    pub praised: u32,
    #[serde(default)]
    pub scolded: u32,
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub failed: u32,
    /// Her emotion when it was over, off even.
    #[serde(default)]
    pub felt: Option<f64>,
    /// Something they did that got to her, this time.
    #[serde(default)]
    pub hurt: Option<Weight>,
    /// They made right something that had got to her.
    #[serde(default)]
    pub mended: bool,
    /// The first time they ever talked with her.
    #[serde(default)]
    pub first_time: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalkKind {
    Private,
    Group,
    Work,
}

/// What she is facing now: left out of what is told, since it is in front
/// of her already.
#[derive(Debug, Clone, Copy)]
pub struct Facing<'a> {
    pub person: i32,
    /// The group's venue, in a group.
    pub group: Option<&'a str>,
}

/// Her talks with others since `since` (when she got up), newest first.
/// `first_met` are the people who first talked with her since then.
pub fn talks(
    heard: &[Heard],
    moved: &[Moved],
    stung: &[Stung],
    first_met: &[i32],
    since: DateTime<Utc>,
    facing: Facing<'_>,
) -> Vec<Talk> {
    let left_out = |person: i32, place: &Place| {
        person == facing.person
            || matches!((place, facing.group), (Place::Group(here), Some(there)) if here == there)
    };
    let mut heard: Vec<&Heard> = heard
        .iter()
        .filter(|heard| heard.at >= since && !left_out(heard.person, &heard.place))
        .collect();
    heard.sort_by_key(|heard| (heard.person, place_key(&heard.place), heard.at));

    struct Span {
        person: i32,
        place: Place,
        talk: Talk,
    }
    let mut spans: Vec<Span> = Vec::new();
    for message in heard {
        match spans.last_mut() {
            Some(span)
                if span.person == message.person
                    && span.place == message.place
                    && message.at - span.talk.ended <= TALK_GAP =>
            {
                span.talk.ended = message.at;
            }
            _ => spans.push(Span {
                person: message.person,
                place: message.place.clone(),
                talk: empty(kind_of(&message.place), message.at),
            }),
        }
    }

    // Each change of her affect goes to the talk it happened in; a task's
    // outcome with no talk around it is a talk of its own.
    let mut moved: Vec<&Moved> = moved
        .iter()
        .filter(|moved| moved.at >= since && moved.person != facing.person)
        .collect();
    moved.sort_by_key(|moved| moved.at);
    for change in moved {
        // Changes do not say where: the latest talk with them it falls in.
        let span = spans
            .iter_mut()
            .filter(|span| {
                span.person == change.person
                    && change.at >= span.talk.started
                    && change.at <= span.talk.ended + SETTLES_WITHIN
            })
            .max_by_key(|span| span.talk.started);
        let talk = match (span, change.cause) {
            (Some(span), _) => &mut span.talk,
            (None, Cause::TaskDone | Cause::TaskFailed) => {
                spans.push(Span {
                    person: change.person,
                    place: Place::Work,
                    talk: empty(TalkKind::Work, change.at),
                });
                &mut spans.last_mut().expect("just pushed").talk
            }
            (None, _) => continue,
        };
        match change.cause {
            Cause::Praise => talk.praised += 1,
            Cause::Scold => talk.scolded += 1,
            Cause::TaskDone => talk.done += 1,
            Cause::TaskFailed => talk.failed += 1,
            Cause::Turn | Cause::Appraisal | Cause::Other => {}
        }
        talk.felt = Some(change.felt_after);
    }

    fn talk_at<'a>(spans: &'a mut [Span], sore: &Stung, at: DateTime<Utc>) -> Option<&'a mut Talk> {
        spans
            .iter_mut()
            .filter(|span| span.person == sore.person && span.place == sore.place)
            .max_by_key(|span| {
                let falls_in = at >= span.talk.started && at <= span.talk.ended + SETTLES_WITHIN;
                (falls_in, span.talk.ended)
            })
            .map(|span| &mut span.talk)
    }
    // What got to her, and what was made right, goes to the talk with them
    // where it happened: the one it falls in, else their last one there.
    for sore in stung.iter().filter(|sore| sore.person != facing.person) {
        if sore.since >= since
            && let Some(talk) = talk_at(&mut spans, sore, sore.since)
        {
            talk.hurt = talk.hurt.max(Some(sore.weight));
        }
        if let Some(mended) = sore.mended.filter(|mended| *mended >= since)
            && let Some(talk) = talk_at(&mut spans, sore, mended)
        {
            talk.mended = true;
        }
    }
    for person in first_met {
        if let Some(span) = spans
            .iter_mut()
            .filter(|span| span.person == *person && span.place != Place::Work)
            .min_by_key(|span| span.talk.started)
        {
            span.talk.first_time = true;
        }
    }

    let mut talks: Vec<Talk> = spans.into_iter().map(|span| span.talk).collect();
    talks.sort_by_key(|talk| std::cmp::Reverse(talk.ended));
    talks
}

/// Her talks with others since she got up, as facts, in private: roughly
/// when, where (the kind only), roughly how long, what happened and how it
/// left her. Nothing about who, and no exact times or counts that could
/// point at someone. The talks that left something are told one by one;
/// the rest only counted, so a plain day does not crowd her.
pub fn section(talks: &[Talk], now: DateTime<Utc>) -> String {
    const HEAD: &str = "## Since you got up";
    if talks.is_empty() {
        return format!("{HEAD}\nNobody else has talked with you since you got up.");
    }
    let (told_one_by_one, counted): (Vec<&Talk>, Vec<&Talk>) =
        talks.iter().partition(|talk| left_something(talk));
    let (told_one_by_one, more) =
        told_one_by_one.split_at(told_one_by_one.len().min(TOLD_ONE_BY_ONE));
    let mut lines = vec![
        "Your talks with others since you got up: only how they went, not who or what about. \
         None of it was the one in front of you. How you are shows, as it would on \
         anyone. What happened, and that it was with someone, they know nothing of unless \
         you tell them; whether you would, and to whom, is as your personality would have \
         it."
        .to_string(),
    ];
    lines.extend(
        told_one_by_one
            .iter()
            .map(|talk| format!("- {}", told(talk, now))),
    );
    let rest: Vec<&Talk> = more.iter().copied().chain(counted).collect();
    if !rest.is_empty() {
        let besides = if told_one_by_one.is_empty() {
            ""
        } else {
            "Besides these: "
        };
        lines.push(format!("- {besides}{}.", together(&rest)));
    }
    format!("{HEAD}\n{}", lines.join("\n"))
}

/// One push of her emotion toward anyone since she got up (praise, a jab,
/// how their words read to her, a task done or failed): how far it moved
/// her, off even. Who and what about stay where they happened.
#[derive(Debug, Clone, Copy)]
pub struct Push {
    pub at: DateTime<Utc>,
    pub valence: f64,
}

/// How long what happened stays with her, as a time constant in hours. A
/// mood carries over (Kuppens et al. 2010); how long is a guess, not
/// measured.
pub const STAYS_FOR_HOURS: f64 = 2.0;

/// How today has left her so far, not toward anyone: every push since she
/// got up, the older ones fainter. Off even, like an emotion; 0 is even.
pub fn own_feeling(pushes: &[Push], now: DateTime<Utc>) -> f64 {
    pushes
        .iter()
        .filter(|push| push.at <= now && push.valence.is_finite())
        .map(|push| {
            let hours = (now - push.at).num_seconds() as f64 / 3600.0;
            push.valence * (-hours / STAYS_FOR_HOURS).exp()
        })
        .sum()
}

/// [`own_feeling`] in words, when it says something.
pub fn own_feeling_words(feeling: f64) -> Option<&'static str> {
    if feeling >= STRONG {
        Some("in really good spirits")
    } else if feeling >= CLEAR {
        Some("a little pleased")
    } else if feeling <= -STRONG {
        Some("hurting")
    } else if feeling <= -CLEAR {
        Some("a little stung")
    } else {
        None
    }
}

/// Low enough that it narrows her (as tiredness does).
pub fn own_feeling_is_low(feeling: f64) -> bool {
    feeling <= -CLEAR
}

/// Whether a talk left her with something worth telling on its own.
fn left_something(talk: &Talk) -> bool {
    talk.first_time
        || talk.praised > 0
        || talk.scolded > 0
        || talk.hurt.is_some()
        || talk.mended
        || talk.done > 0
        || talk.failed > 0
        || talk.felt.is_some_and(|felt| felt.abs() >= CLEAR)
}

/// How a talk left her, a hurt in it counting as at least that much sting.
fn felt_with_hurt(talk: &Talk) -> f64 {
    let felt = talk.felt.unwrap_or(0.0);
    match talk.hurt {
        Some(Weight::Petty) => felt.min(-CLEAR),
        Some(Weight::Hurt) => felt.min(-STRONG),
        Some(Weight::Deep) => felt.min(-2.0 * STRONG),
        None => felt,
    }
}

fn told(talk: &Talk, now: DateTime<Utc>) -> String {
    let ago = rough_ago(now - talk.ended);
    let mut what = Vec::new();
    if talk.first_time {
        what.push("someone you had never talked with before".to_string());
    }
    if talk.praised > 0 {
        what.push("they praised you".to_string());
    }
    if talk.scolded > 0 {
        what.push("they scolded you".to_string());
    }
    if let Some(weight) = talk.hurt {
        what.push(
            match weight {
                Weight::Petty => "something they did annoyed you a little",
                Weight::Hurt => "something they did got to you",
                Weight::Deep => "something they did really hurt",
            }
            .to_string(),
        );
    }
    if talk.mended {
        what.push("they made right something that had got to you".to_string());
    }
    let head = match talk.kind {
        TalkKind::Work => {
            let outcome = match (talk.done, talk.failed) {
                (0, 0) => "you worked on something someone asked of you",
                (0, _) => "something someone asked of you did not work out",
                (_, 0) => "you got done something someone asked of you",
                _ => "something someone asked of you partly worked out",
            };
            format!("{ago}, {outcome}")
        }
        kind => {
            let place = match kind {
                TalkKind::Group => "in a group",
                _ => "a private talk",
            };
            format!(
                "{ago}, {place}, {}",
                rough_length(talk.ended - talk.started)
            )
        }
    };
    let mut line = head;
    if !what.is_empty() {
        line.push_str("; ");
        line.push_str(&what.join("; "));
    }
    if let Some(felt) = talk.felt.and_then(left_her) {
        line.push_str("; ");
        line.push_str(felt);
    }
    line.push('.');
    capitalized(&line)
}

/// Talks counted together, by kind, with how many of them left her pleased
/// or stung.
fn together(talks: &[&Talk]) -> String {
    let count = |kind: TalkKind| talks.iter().filter(|talk| talk.kind == kind).count();
    let mut parts = Vec::new();
    for (kind, one, many) in [
        (TalkKind::Private, "private talk", "private talks"),
        (TalkKind::Group, "talk in a group", "talks in groups"),
        (
            TalkKind::Work,
            "thing done for someone",
            "things done for someone",
        ),
    ] {
        match count(kind) {
            0 => {}
            1 => parts.push(format!("1 {one}")),
            n => parts.push(format!("{n} {many}")),
        }
    }
    let glad = talks
        .iter()
        .filter(|talk| felt_with_hurt(talk) >= CLEAR)
        .count();
    let sore = talks
        .iter()
        .filter(|talk| felt_with_hurt(talk) <= -CLEAR)
        .count();
    let mut text = parts.join(", ");
    match (glad, sore) {
        (0, 0) => text.push_str(", nothing much in them"),
        (glad, 0) => text.push_str(&format!(" ({glad} left you pleased)")),
        (0, sore) => text.push_str(&format!(" ({sore} stung)")),
        (glad, sore) => text.push_str(&format!(" ({glad} left you pleased, {sore} stung)")),
    }
    text
}

fn left_her(felt: f64) -> Option<&'static str> {
    if felt >= STRONG {
        Some("at the time you came away in really good spirits")
    } else if felt >= CLEAR {
        Some("at the time you came away a little pleased")
    } else if felt <= -STRONG {
        Some("at the time you came away hurting")
    } else if felt <= -CLEAR {
        Some("at the time you came away a little stung")
    } else {
        None
    }
}

/// When, no closer than it needs to be.
fn rough_ago(ago: Duration) -> &'static str {
    match ago.num_minutes().max(0) {
        0..15 => "just now",
        15..60 => "within the last hour",
        60..180 => "an hour or two ago",
        180..360 => "a few hours ago",
        _ => "earlier today",
    }
}

/// How long, no closer than it needs to be.
fn rough_length(span: Duration) -> &'static str {
    match span.num_minutes().max(0) {
        0..5 => "briefly",
        5..45 => "for a while",
        45..90 => "for about an hour",
        _ => "for hours",
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn kind_of(place: &Place) -> TalkKind {
    match place {
        Place::Private => TalkKind::Private,
        Place::Group(_) => TalkKind::Group,
        Place::Work => TalkKind::Work,
    }
}

fn place_key(place: &Place) -> (u8, &str) {
    match place {
        Place::Private => (0, ""),
        Place::Group(venue) => (1, venue),
        Place::Work => (2, ""),
    }
}

fn empty(kind: TalkKind, at: DateTime<Utc>) -> Talk {
    Talk {
        kind,
        started: at,
        ended: at,
        praised: 0,
        scolded: 0,
        done: 0,
        failed: 0,
        felt: None,
        hurt: None,
        mended: false,
        first_time: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes_ago: i64) -> DateTime<Utc> {
        now() - Duration::minutes(minutes_ago)
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-11T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn heard(person: i32, place: Place, minutes_ago: i64) -> Heard {
        Heard {
            person,
            place,
            at: at(minutes_ago),
        }
    }

    fn moved(person: i32, cause: Cause, minutes_ago: i64, felt_after: f64) -> Moved {
        Moved {
            person,
            at: at(minutes_ago),
            cause,
            felt_after,
        }
    }

    const ME: Facing<'static> = Facing {
        person: 1,
        group: None,
    };

    #[test]
    fn messages_close_together_are_one_talk_and_a_long_gap_starts_another() {
        let heard = [
            heard(2, Place::Private, 200),
            heard(2, Place::Private, 190),
            heard(2, Place::Private, 60),
            heard(2, Place::Private, 50),
            heard(2, Place::Private, 40),
        ];
        let talks = talks(&heard, &[], &[], &[], at(600), ME);
        assert_eq!(talks.len(), 2);
        assert_eq!(
            talks[0].ended - talks[0].started,
            Duration::minutes(20),
            "newest first"
        );
        assert_eq!(talks[1].ended - talks[1].started, Duration::minutes(10));
    }

    #[test]
    fn the_one_she_faces_and_the_group_she_is_in_are_left_out() {
        let heard = [
            heard(1, Place::Private, 30),
            heard(3, Place::Group("telegram:-100".into()), 20),
            heard(4, Place::Group("telegram:-200".into()), 10),
        ];
        let moved = [moved(1, Cause::Praise, 30, 30.0)];
        let in_group = Facing {
            person: 1,
            group: Some("telegram:-100"),
        };
        let talks = talks(&heard, &moved, &[], &[], at(600), in_group);
        assert_eq!(talks.len(), 1, "{talks:?}");
        assert_eq!(talks[0].kind, TalkKind::Group);
        assert_eq!(talks[0].praised, 0, "the praise was the one she faces");
    }

    #[test]
    fn before_she_got_up_is_not_told() {
        let heard = [heard(2, Place::Private, 700), heard(3, Place::Private, 30)];
        let talks = talks(&heard, &[], &[], &[], at(600), ME);
        assert_eq!(talks.len(), 1);
    }

    #[test]
    fn what_moved_her_lands_in_the_talk_it_happened_in() {
        let heard = [heard(2, Place::Private, 40), heard(2, Place::Private, 30)];
        let moved = [
            moved(2, Cause::Turn, 40, 0.0),
            moved(2, Cause::Praise, 30, 18.0),
            // Her appraisal of their last words lands a little later.
            moved(2, Cause::Appraisal, 27, 28.0),
            // A task done later, with no talk around it, is its own.
            moved(2, Cause::TaskFailed, 5, -12.0),
        ];
        let talks = talks(&heard, &moved, &[], &[], at(600), ME);
        assert_eq!(talks.len(), 2, "{talks:?}");
        assert_eq!(talks[0].kind, TalkKind::Work);
        assert_eq!(talks[0].failed, 1);
        assert_eq!(talks[1].praised, 1);
        assert_eq!(
            talks[1].felt,
            Some(28.0),
            "how it left her is the last change"
        );
    }

    #[test]
    fn hurts_and_first_meetings_go_with_that_persons_talk() {
        let heard = [heard(2, Place::Private, 90), heard(3, Place::Private, 20)];
        let stung = [
            Stung {
                person: 2,
                place: Place::Private,
                weight: Weight::Deep,
                since: at(85),
                mended: None,
            },
            // Made right today, born before she got up.
            Stung {
                person: 3,
                place: Place::Private,
                weight: Weight::Hurt,
                since: at(2000),
                mended: Some(at(20)),
            },
        ];
        let talks = talks(&heard, &[], &stung, &[3], at(600), ME);
        assert_eq!(talks[0].hurt, None);
        assert!(talks[0].mended && talks[0].first_time);
        assert_eq!(talks[1].hurt, Some(Weight::Deep));
    }

    #[test]
    fn talks_that_left_something_are_told_roughly_and_the_rest_counted() {
        let talks = [
            Talk {
                hurt: Some(Weight::Hurt),
                felt: Some(-15.0),
                ..empty(TalkKind::Group, at(10))
            },
            // Nothing in it: counted, not told.
            Talk {
                ended: at(20),
                felt: Some(3.0),
                ..empty(TalkKind::Private, at(30))
            },
            Talk {
                ended: at(45),
                praised: 2,
                felt: Some(30.0),
                ..empty(TalkKind::Private, at(95))
            },
            Talk {
                done: 1,
                ..empty(TalkKind::Work, at(200))
            },
        ];
        let text = section(&talks, now());
        assert!(text.starts_with("## Since you got up\n"), "{text}");
        let told: Vec<&str> = text.lines().filter(|line| line.starts_with("- ")).collect();
        assert_eq!(
            told,
            [
                "- Just now, in a group, briefly; something they did got to you; \
                 at the time you came away a little stung.",
                "- Within the last hour, a private talk, for about an hour; they praised you; \
                 at the time you came away in really good spirits.",
                "- A few hours ago, you got done something someone asked of you.",
                "- Besides these: 1 private talk, nothing much in them.",
            ],
            "{text}"
        );
        // No exact times or counts: they could point at someone.
        assert!(
            !text.chars().any(|c| c.is_ascii_digit() && c != '1'),
            "{text}"
        );
    }

    #[test]
    fn a_plain_day_is_only_counted_and_nobody_else_is_a_fact_too() {
        assert_eq!(
            section(&[], now()),
            "## Since you got up\nNobody else has talked with you since you got up."
        );
        let plain = [
            empty(TalkKind::Private, at(30)),
            empty(TalkKind::Group, at(60)),
            empty(TalkKind::Group, at(90)),
        ];
        assert!(
            section(&plain, now())
                .ends_with("- 1 private talk, 2 talks in groups, nothing much in them."),
            "{}",
            section(&plain, now())
        );
        let many: Vec<Talk> = (0..9)
            .map(|index| Talk {
                praised: 1,
                felt: (index == 8).then_some(-30.0),
                ..empty(TalkKind::Private, at(10 + index * 20))
            })
            .collect();
        let text = section(&many, now());
        assert_eq!(
            text.lines().filter(|line| line.starts_with("- ")).count(),
            7
        );
        assert!(
            text.ends_with("- Besides these: 3 private talks (1 stung)."),
            "{text}"
        );
    }

    #[test]
    fn what_today_did_to_her_adds_up_and_fades() {
        let push = |minutes_ago, valence| Push {
            at: at(minutes_ago),
            valence,
        };
        // A jab an hour ago, two compliments since: on balance, pleased.
        let day = [push(60, -30.0), push(20, 20.0), push(5, 15.0)];
        let feeling = own_feeling(&day, now());
        assert_eq!(
            own_feeling_words(feeling),
            Some("a little pleased"),
            "{feeling}"
        );
        // The jab alone, just now: it hurts and narrows her.
        let jab = own_feeling(&[push(1, -30.0)], now());
        assert_eq!(own_feeling_words(jab), Some("hurting"));
        assert!(own_feeling_is_low(jab));
        // Hours later it has faded to nothing.
        let later = now() + Duration::hours(6);
        assert_eq!(
            own_feeling_words(own_feeling(&[push(1, -30.0)], later)),
            None
        );
        assert_eq!(own_feeling(&[], now()), 0.0);
    }
}
