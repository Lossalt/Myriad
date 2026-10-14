//! What she is in the middle of in a group, step by step: taking up a line
//! that called her, glancing at talk that did not, saying something first,
//! and the answer and its going out that each of them may come to. Each is
//! an errand that waits for one thing at a time (a timer, or what it asked
//! the end); everything else happens at once, as it would between two
//! awaits.

use serde::{Deserialize, Serialize};

use super::ledger::{differs, keep_ledger, note_said};
use super::lines::*;
use super::*;
use crate::joining::Why;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Errand {
    pub id: String,
    /// The timer key or the id of what it asked: what it waits for.
    pub awaiting: String,
    pub kind: Kind,
    pub doing: Doing,
}

/// What the errand is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Kind {
    /// A line that called her: seen, taken up, answered, then whoever
    /// waited meanwhile.
    TakeUp {
        #[serde(default)]
        waiter: Option<String>,
        message: Message,
        /// Whether a reply held back may still be said again.
        again: bool,
    },
    /// A glance at talk that did not call her, settled on line `id`.
    Look {
        id: String,
        #[serde(default)]
        message: Option<Message>,
        #[serde(default)]
        why: Option<Why>,
        /// The answer it is on is the one she chose to say, not one that
        /// waited.
        #[serde(default)]
        spoke: bool,
    },
    /// Five minutes on, whether anyone took up her speaking up.
    SpokeUp { at: i64, line: String },
    /// Saying something first.
    First {
        waiter: String,
        reason: String,
        #[serde(default)]
        owner: Option<i32>,
        #[serde(default)]
        spoke: Option<bool>,
    },
}

/// Where the errand is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "at")]
pub enum Doing {
    /// Until she sees the line.
    Seeing,
    /// The group just had her reply: a moment before the turn.
    Beginning,
    Answering(Box<Answering>),
    /// Done with one: a pause before the next that waited.
    Pausing,
    /// Before the next that waited, still resting.
    Resting,
    Glancing,
    Settling,
    Judging(Box<Judging>),
    Reacting {
        emoji: String,
    },
    Timing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judging {
    pub message: Message,
    pub stage: JudgeStage,
    #[serde(default)]
    pub owner: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JudgeStage {
    Identifying,
    Looking,
    Judging,
}

/// One answer: from the gates to what went out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answering {
    pub message: Message,
    #[serde(default)]
    pub chime: Option<String>,
    /// Saying something first: nobody's line behind it.
    #[serde(default)]
    pub first: bool,
    pub stage: AnswerStage,
    #[serde(default)]
    pub user_id: Option<i32>,
    #[serde(default)]
    pub owner: Option<i32>,
    #[serde(default)]
    pub stranger: Option<Stranger>,
    #[serde(default)]
    pub began: i64,
    #[serde(default)]
    pub transcript: Vec<Spoken>,
    #[serde(default)]
    pub said: String,
    #[serde(default)]
    pub making_sense: Option<String>,
    #[serde(default)]
    pub room: Option<crate::talk_shape::Shape>,
    #[serde(default)]
    pub noted: Pieces,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub sticker: Option<Value>,
    #[serde(default)]
    pub deliver: Option<Deliver>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnswerStage {
    Claiming,
    Looking,
    Identifying,
    Preparing,
    Sensing,
    LedgerForTurn,
    Turning,
    StrangerLedger,
    StrangerSensing,
    StrangerTyping,
    StrangerTurning,
    Delivering,
}

/// Her reply going out, message by message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deliver {
    pub people: Vec<(String, String)>,
    pub messages: Vec<String>,
    pub index: usize,
    #[serde(default)]
    pub chunks: Vec<String>,
    #[serde(default)]
    pub chunk: usize,
    pub sent: bool,
    pub went_out: Vec<String>,
    pub since: i64,
    pub looked: usize,
    /// What was said to her (empty when nobody said anything).
    pub said: String,
    /// Her whole reply, as written, and who the look at it is billed to.
    pub reply: String,
    pub owner: i32,
    #[serde(default)]
    pub came: usize,
    /// How long she still types, once the mark is shown.
    #[serde(default)]
    pub left: f64,
    pub stage: DeliverStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliverStage {
    TypingMark,
    Typing,
    Fitting,
    Sending,
}

/// What a piece of an errand came to.
enum Next {
    /// Waiting on `awaiting`.
    Wait(String),
    Done(bool),
}

struct Cx<'a> {
    group: &'a mut Group,
    at: &'a mut At,
    out: &'a mut Outcome,
}

impl Cx<'_> {
    fn now(&self) -> i64 {
        self.at.now
    }

    fn id(&mut self) -> String {
        self.group.next += 1;
        format!("g{}", self.group.next)
    }

    fn wake(&mut self, after: f64) -> String {
        let key = self.id();
        self.out.effects.push(Effect::Wake {
            key: key.clone(),
            after,
        });
        key
    }

    fn effect(&mut self, make: impl FnOnce(String) -> Effect) -> String {
        let id = self.id();
        self.out.effects.push(make(id.clone()));
        id
    }

    fn settle(&mut self, waiter: &Option<String>, value: bool) {
        if let Some(waiter) = waiter {
            self.out.effects.push(Effect::Settled {
                waiter: waiter.clone(),
                value,
            });
        }
    }
}

pub(super) fn step(group: &mut Group, at: &mut At, event: Event, out: &mut Outcome) {
    let mut cx = Cx { group, at, out };
    match event {
        Event::Restored { lines, spoke_up } => restored(&mut cx, lines, spoke_up),
        Event::Reached { message, seen_at } => {
            if cx.group.reach.is_none() {
                cx.group.reach = Some(message);
                cx.group.seen_before = Some(seen_at);
            }
        }
        Event::Heard { message, restore } => {
            let fresh = record(&mut cx, &message, restore);
            cx.out.fresh = Some(fresh);
        }
        Event::Called { message, waiter } => handle(&mut cx, message, waiter),
        Event::Noticed { message } => notice(&mut cx, message),
        Event::Muted {
            by,
            mute,
            everyone,
            restore,
        } => muted(&mut cx, &by, mute, everyone, restore),
        Event::Resumed {
            why,
            past,
            up_since,
        } => resumed(&mut cx, why, past, up_since),
        Event::SpeakFirst {
            reason,
            owner,
            waiter,
        } => speak_first(&mut cx, reason, owner, waiter),
        Event::StickerSent { meaning } => {
            let now = cx.now();
            record_hers(cx.group, &format!("（表情包：{meaning}）"), now, cx.out);
        }
        Event::Woke { key } => resume(&mut cx, &key, None),
        Event::Done { id, answer } => resume(&mut cx, &id, Some(answer)),
    }
}

// ---------------------------------------------------------------- state

fn restored(cx: &mut Cx, lines: Option<Vec<Line>>, kept: Vec<SpokeUp>) {
    if cx.group.restored {
        return;
    }
    cx.group.restored = true;
    let now = cx.now();
    if let Some(lines) = lines {
        merge_restored(cx.group, lines, now);
    }
    let since: Vec<SpokeUp> = cx.group.spoke_up.drain(..).collect();
    cx.group.spoke_up.extend(kept);
    cx.group.spoke_up.extend(since);
    while cx.group.spoke_up.len() > SPOKE_UP_KEPT {
        cx.group.spoke_up.pop_front();
    }
}

pub(super) fn in_talk(group: &Group, now: i64) -> bool {
    group
        .called
        .is_some_and(|at| now - at < seconds(IN_TALK_SECONDS))
        || group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .is_some_and(|line| {
                let age = now - line.at_micros();
                age >= 0 && age < seconds(IN_TALK_SECONDS)
            })
}

pub(super) fn muted_now(group: &Group, now: i64) -> bool {
    [group.muted_until, group.everyone_muted_until]
        .into_iter()
        .flatten()
        .any(|until| until > now)
}

fn stopped_answering(group: &mut Group, message: &Message, now: i64) -> bool {
    group
        .paused
        .retain(|_, since| now - *since < seconds(LOOP_PAUSE_SECONDS));
    group.paused.contains_key(&message.from)
}

fn answered(group: &mut Group, message: &Message, now: i64) {
    group.answered.push_back(Answered {
        from: message.from.clone(),
        at: now,
    });
    while group.answered.len() > LOOP_ROUNDS {
        group.answered.pop_front();
    }
    let looping = group.answered.len() == LOOP_ROUNDS
        && group.answered.iter().all(|one| one.from == message.from)
        && group
            .answered
            .front()
            .is_some_and(|one| now - one.at < seconds(LOOP_WINDOW_SECONDS));
    if looping {
        group.paused.insert(message.from.clone(), now);
        group.answered.clear();
    }
}

pub(super) fn taken_up(group: &mut Group, now: i64) {
    if let Some(last) = group.spoke_up.back_mut()
        && now - last.at < seconds(TAKEN_UP_WITHIN_SECONDS)
    {
        last.taken = true;
    }
}

/// Count one more of today's, unless `limit` is reached.
pub(super) fn count_today(slot: &mut Option<DayCount>, today: &str, limit: u32) -> bool {
    let count = match slot {
        Some(day) if day.day == today => day.count,
        _ => 0,
    };
    if count >= limit {
        return false;
    }
    *slot = Some(DayCount {
        day: today.to_string(),
        count: count + 1,
    });
    true
}

pub(super) fn begin(group: &mut Group, now: i64) -> Result<(), Option<f64>> {
    if group.busy {
        return Err(None);
    }
    if let Some(at) = group.last_reply {
        let rest = seconds(GROUP_PAUSE_SECONDS) - (now - at);
        if rest > 0 {
            return Err(Some(rest as f64 / 1_000_000.0));
        }
    }
    group.busy = true;
    Ok(())
}

pub(super) fn park(group: &mut Group, message: Message) {
    group.waiting.push_back(message);
    while group.waiting.len() > WAITING_LINES {
        group.waiting.pop_front();
    }
}

pub(super) fn end_turn(group: &mut Group, replied: bool, now: i64) {
    group.busy = false;
    if replied {
        group.last_reply = Some(now);
    }
}

/// What she does when the talk is quiet enough or she is free: where she
/// is in her day.
fn where_she_is(at: &At, in_talk: bool, may_sleep: bool) -> crate::timing::Where {
    use crate::timing::Where;
    if in_talk {
        return Where::InTalk;
    }
    if may_sleep && let Some(wakes_in) = at.her.asleep_for {
        return Where::Asleep { wakes_in };
    }
    match at
        .her
        .busy_for
        .map(|free| free.trunc())
        .filter(|free| *free > 0.0)
    {
        Some(free_in) => Where::Busy { free_in },
        None => Where::Free,
    }
}

fn spoke_up_now(cx: &mut Cx) {
    let now = cx.now();
    cx.group.spoke_up.push_back(SpokeUp {
        at: now,
        taken: false,
    });
    while cx.group.spoke_up.len() > SPOKE_UP_KEPT {
        cx.group.spoke_up.pop_front();
    }
    let Some(line) = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| line.hers)
        .map(|line| line.text.clone())
    else {
        return;
    };
    let id = cx.id();
    let key = cx.wake(TAKEN_UP_WITHIN_SECONDS);
    cx.group.errands.push(Errand {
        id,
        awaiting: key,
        kind: Kind::SpokeUp { at: now, line },
        doing: Doing::Timing,
    });
}

// ---------------------------------------------------------------- lines in

fn record(cx: &mut Cx, message: &Message, restore: Option<Restore>) -> bool {
    let now = cx.now();
    let people = people(cx.group, now);
    if let Some(restore) = restore {
        restored(cx, restore.lines, restore.spoke_up);
    }
    let line = Line {
        at: time(now),
        message_id: Some(message.message_id.clone()),
        name: message.display_name.clone(),
        from: Some(message.from.clone()),
        text: bounded(&by_name(&message.said(), &people)),
        hers: false,
        addressed: message.addressed,
        images: message.images.clone(),
        seen: Vec::new(),
    };
    if !remember_line(cx.group, line, now, cx.out) {
        return false;
    }
    // Written down as a group she is in, now and then.
    let due = cx
        .group
        .reach_kept
        .is_none_or(|kept| now - kept >= seconds(REACH_KEPT_EVERY_SECONDS));
    if due {
        cx.group.reach_kept = Some(now);
        cx.out.effects.push(Effect::KeepReach {
            thread: message.thread,
            seen_at: now,
        });
    }
    let typed = crate::talk_shape::typed_by(
        &super::ledger_who(&message.from),
        lines::time(now).timestamp(),
        &message.text,
    );
    if note_said(cx.group, typed, None, Some(&message.text)) {
        keep_ledger(cx.group, cx.out);
    }
    // Not on this end, she hears nothing there (D3-2).
    let away = cx.at.her.away;
    if !away {
        cx.out.effects.push(Effect::Jargon {
            text: message.text.clone(),
        });
    }
    take_in(cx);
    if cx.group.pictures.len() > PICTURES_KEPT {
        cx.group.pictures.clear();
    }
    for image in &message.images {
        *cx.group.pictures.entry(image.key.clone()).or_default() += 1;
    }
    true
}

/// Once enough of the talk has gone by, what she heard in it.
fn take_in(cx: &mut Cx) {
    let now = cx.now();
    if cx.at.her.away {
        cx.group.heard_upto = cx
            .group
            .lines
            .back()
            .map(|line| line.at_micros())
            .or(cx.group.heard_upto);
        return;
    }
    let upto = cx.group.heard_upto;
    let fresh: Vec<&Line> = cx
        .group
        .lines
        .iter()
        .filter(|line| upto.is_none_or(|upto| line.at_micros() > upto))
        .collect();
    let theirs = fresh.iter().filter(|line| !line.hers).count();
    let waited = fresh
        .first()
        .is_some_and(|line| now - line.at_micros() >= HEARD_AFTER_SECONDS * 1_000_000);
    if theirs < HEARD_EVERY && !(waited && theirs >= HEARD_AT_LEAST) {
        return;
    }
    let stretch: Vec<Stretch> = fresh
        .iter()
        .map(|line| Stretch {
            at: line.at.fixed_offset().to_rfc3339(),
            name: line.name.clone(),
            text: line.text.clone(),
            hers: line.hers,
        })
        .collect();
    cx.group.heard_upto = fresh.last().map(|line| line.at_micros());
    cx.out.effects.push(Effect::TakeIn { stretch });
}

pub(super) fn first_time(group: &mut Group, message_id: &str) -> bool {
    if message_id.is_empty() {
        return true;
    }
    if group.handled.iter().any(|id| id == message_id) {
        return false;
    }
    group.handled.push_back(message_id.to_string());
    while group.handled.len() > HANDLED_KEPT {
        group.handled.pop_front();
    }
    true
}

fn handle(cx: &mut Cx, message: Message, waiter: Option<String>) {
    let now = cx.now();
    // She is not on this end: not in the group either (D3-2).
    if cx.at.her.away {
        cx.group.reach = Some(message.clone());
        cx.group.left_while_away = Some((message, now));
        cx.settle(&waiter, false);
        return;
    }
    if !first_time(cx.group, &message.message_id) {
        cx.settle(&waiter, false);
        return;
    }
    taken_up(cx.group, now);
    cx.group.reach = Some(message.clone());
    let talking = in_talk(cx.group, now);
    let at = where_she_is(cx.at, talking, true);
    let wait = crate::timing::seen_after(at, cx.at.rolls.roll(), cx.at.rolls.roll())
        + crate::timing::reading(message.text.chars().count());
    // A long wait goes on apart from whoever handed the line in.
    let waiter = if wait > HOLD_AT_MOST_SECONDS {
        cx.settle(&waiter, true);
        None
    } else {
        waiter
    };
    let id = cx.id();
    let key = cx.wake(wait);
    cx.group.errands.push(Errand {
        id,
        awaiting: key,
        kind: Kind::TakeUp {
            waiter,
            message,
            again: true,
        },
        doing: Doing::Seeing,
    });
}

fn glance_after(at: &mut At) -> f64 {
    if let crate::timing::Where::Asleep { wakes_in } = where_she_is(at, false, true) {
        return wakes_in + at.rolls.range(GLANCE_AFTER_SECONDS) as f64;
    }
    let free_in = at
        .her
        .busy_for
        .filter(|free| *free > 0.0)
        .unwrap_or_default()
        .min(LONGEST_BUSY_SECONDS);
    free_in + at.rolls.range(GLANCE_AFTER_SECONDS) as f64
}

fn notice(cx: &mut Cx, message: Message) {
    if cx.at.her.away {
        return;
    }
    let now = cx.now();
    let id = message.message_id.clone();
    cx.group.unjudged_since.get_or_insert(now);
    cx.group.reach = Some(message.clone());
    cx.group.pending = Some(message);
    let after = if in_talk(cx.group, now) {
        SETTLE_SECONDS
    } else if cx.group.glance_at.is_some() {
        return;
    } else {
        let after = glance_after(cx.at);
        cx.group.glance_at = Some(now + seconds(after));
        after
    };
    let errand = cx.id();
    let key = cx.wake(after);
    cx.group.errands.push(Errand {
        id: errand,
        awaiting: key,
        kind: Kind::Look {
            id,
            message: None,
            why: None,
            spoke: false,
        },
        doing: Doing::Glancing,
    });
}

fn told(mute: Mute, everyone: bool) -> String {
    fn how_long(seconds: u64) -> String {
        let minutes = seconds.div_ceil(60);
        match minutes {
            0..=89 => format!(" {minutes} 分钟"),
            90..=2159 => format!(" {} 小时", (minutes + 30) / 60),
            _ => format!(" {} 天", (minutes + 720) / 1440),
        }
    }
    match (mute, everyone) {
        (Mute::For { seconds }, false) => format!("（把你禁言了{}）", how_long(seconds)),
        (Mute::For { seconds }, true) => format!("（全员禁言{}）", how_long(seconds)),
        (Mute::UntilLifted, false) => "（把你禁言了）".to_string(),
        (Mute::UntilLifted, true) => "（开了全员禁言）".to_string(),
        (Mute::Lifted, false) => "（解除了你的禁言）".to_string(),
        (Mute::Lifted, true) => "（解除了全员禁言）".to_string(),
    }
}

/// Muted with no end given: as long as a group can mute anyone.
const UNTIL_LIFTED_DAYS: i64 = 30;

fn muted(cx: &mut Cx, by: &str, mute: Mute, everyone: bool, restore: Option<Restore>) {
    let now = cx.now();
    let until = match mute {
        Mute::For { seconds } => {
            Some(now + (seconds.min(i64::MAX as u64 / 1_000_000) as i64) * 1_000_000)
        }
        Mute::UntilLifted => Some(now + UNTIL_LIFTED_DAYS * 86_400 * 1_000_000),
        Mute::Lifted => None,
    };
    if everyone {
        cx.group.everyone_muted_until = until;
    } else {
        cx.group.muted_until = until;
    }
    let name = (!by.is_empty())
        .then(|| {
            people(cx.group, now)
                .into_iter()
                .find(|(_, from)| from == by)
                .map(|(name, _)| name)
        })
        .flatten()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "群管理".to_string());
    if let Some(restore) = restore {
        restored(cx, restore.lines, restore.spoke_up);
    }
    let line = Line {
        at: time(now),
        message_id: None,
        name,
        from: (!by.is_empty()).then(|| by.to_string()),
        text: told(mute, everyone),
        hers: false,
        addressed: false,
        images: Vec::new(),
        seen: Vec::new(),
    };
    remember_line(cx.group, line, now, cx.out);
}

/// The line that called her before `up_since` and that she never answered,
/// out of what the group said meanwhile: the latest, if she has not spoken
/// there since and it is recent enough to still be talk. Each line once.
fn left_waiting(group: &mut Group, past: &[Past], up_since: i64, now: i64) -> Option<Message> {
    let before = up_since - 5 * 1_000_000;
    let mut past: Vec<&Past> = past.iter().collect();
    past.sort_by_key(|line| line.at);
    let after_her = past
        .iter()
        .rposition(|line| line.hers)
        .map_or(0, |at| at + 1);
    let line = past[after_her..].iter().rev().find(|line| {
        !line.hers
            && line.message.addressed
            && line.at < before
            && now - line.at < TRANSCRIPT_FOR_SECONDS * 1_000_000
    })?;
    let key = line.message.message_id.clone();
    if group.taken_back.contains(&key) {
        return None;
    }
    group.taken_back.push_back(key);
    while group.taken_back.len() > WAITING_LINES {
        group.taken_back.pop_front();
    }
    Some(line.message.clone())
}

fn resumed(cx: &mut Cx, why: Resume, past: Vec<Past>, up_since: Option<i64>) {
    let now = cx.now();
    match why {
        Resume::Back => {
            let left = cx
                .group
                .left_while_away
                .take()
                .filter(|(_, at)| now - *at < TRANSCRIPT_FOR_SECONDS * 1_000_000);
            if let Some((line, _)) = left {
                handle(cx, line, None);
            }
        }
        Resume::Restart => {
            let waiting = left_waiting(cx.group, &past, up_since.unwrap_or(now), now);
            let people = people(cx.group, now);
            let fresh: Vec<Line> = past
                .into_iter()
                .filter(|past| {
                    !cx.group.lines.iter().any(|line| {
                        line.message_id.as_deref() == Some(past.message.message_id.as_str())
                    })
                })
                .map(|past| Line {
                    at: time(past.at),
                    message_id: Some(past.message.message_id.clone()),
                    name: if past.hers {
                        String::new()
                    } else {
                        past.message.display_name.clone()
                    },
                    from: (!past.hers).then(|| past.message.from.clone()),
                    text: bounded(&by_name(&past.message.said(), &people)),
                    hers: past.hers,
                    addressed: past.message.addressed,
                    images: if past.hers {
                        Vec::new()
                    } else {
                        past.message.images
                    },
                    seen: Vec::new(),
                })
                .filter(|line| within(line, now))
                .collect();
            if !fresh.is_empty() {
                for line in &fresh {
                    let by = if line.hers {
                        HER.to_string()
                    } else {
                        super::ledger_who(line.from.as_deref().unwrap_or_default())
                    };
                    let typed = crate::talk_shape::typed_by(&by, line.at.timestamp(), &line.text);
                    note_said(cx.group, typed, None, Some(&line.text));
                }
                merge_restored(cx.group, fresh, now);
                cx.out.effects.push(Effect::KeepLines {
                    lines: cx.group.lines.iter().cloned().collect(),
                });
                keep_ledger(cx.group, cx.out);
            }
            if let Some(line) = waiting {
                handle(cx, line, None);
            }
        }
    }
}

// ---------------------------------------------------------------- errands

fn resume(cx: &mut Cx, key: &str, answer: Option<Answer>) {
    let Some(index) = cx
        .group
        .errands
        .iter()
        .position(|errand| errand.awaiting == key)
    else {
        return;
    };
    let errand = cx.group.errands.remove(index);
    advance(cx, errand, answer);
}

/// Keep `errand` waiting on what `next` says, or let it go.
fn park_errand(cx: &mut Cx, mut errand: Errand, awaiting: String) {
    errand.awaiting = awaiting;
    cx.group.errands.push(errand);
}

fn advance(cx: &mut Cx, mut errand: Errand, answer: Option<Answer>) {
    let doing = std::mem::replace(&mut errand.doing, Doing::Timing);
    match doing {
        Doing::Seeing => {
            // She sees the line: called there now.
            cx.group.called = Some(cx.now());
            take_up_begin(cx, errand);
        }
        Doing::Beginning => take_up_begin(cx, errand),
        Doing::Answering(mut answering) => match answer_step(cx, &mut answering, answer) {
            Next::Wait(awaiting) => {
                errand.doing = Doing::Answering(answering);
                park_errand(cx, errand, awaiting);
            }
            Next::Done(replied) => answered_then(cx, errand, replied),
        },
        Doing::Pausing | Doing::Resting => finish_begin(cx, errand),
        Doing::Glancing => {
            let now = cx.now();
            let Kind::Look { id, .. } = &mut errand.kind else {
                return;
            };
            // A glance takes in whatever is latest by then.
            if cx.group.glance_at.is_some_and(|at| at <= now) {
                cx.group.glance_at = None;
                match cx
                    .group
                    .pending
                    .as_ref()
                    .map(|line| line.message_id.clone())
                {
                    Some(latest) => *id = latest,
                    None => return,
                }
            }
            look_loop(cx, errand);
        }
        Doing::Settling => look_loop(cx, errand),
        Doing::Judging(mut judging) => match judge_step(cx, &mut judging, answer) {
            Ok(awaiting) => {
                errand.doing = Doing::Judging(judging);
                park_errand(cx, errand, awaiting);
            }
            Err(decided) => judged(cx, errand, decided),
        },
        Doing::Reacting { emoji } => {
            if let (
                Some(Answer::Reacted { ok: true }),
                Kind::Look {
                    message: Some(message),
                    ..
                },
            ) = (answer, &errand.kind)
            {
                let now = cx.now();
                let text = format!("（给 {} 那句贴了 {emoji}）", message.display_name);
                record_hers(cx.group, &text, now, cx.out);
            }
            chain_end(cx, errand);
        }
        Doing::Timing => {
            if let Kind::SpokeUp { at, line } = errand.kind {
                let taken = cx
                    .group
                    .spoke_up
                    .iter()
                    .find(|one| one.at == at)
                    .is_some_and(|one| one.taken);
                cx.out.effects.push(Effect::SpokeUp { line, taken });
            }
        }
    }
}

fn take_up_begin(cx: &mut Cx, mut errand: Errand) {
    let now = cx.now();
    let Kind::TakeUp {
        message, waiter, ..
    } = &errand.kind
    else {
        return;
    };
    match begin(cx.group, now) {
        Ok(()) => {
            let message = message.clone();
            cx.group.held_back = false;
            start_answer(cx, errand, message, None, false);
        }
        Err(Some(rest)) => {
            let key = cx.wake(rest);
            errand.doing = Doing::Beginning;
            park_errand(cx, errand, key);
        }
        Err(None) => {
            let (message, waiter) = (message.clone(), waiter.clone());
            park(cx.group, message);
            cx.settle(&waiter, true);
        }
    }
}

fn start_answer(
    cx: &mut Cx,
    mut errand: Errand,
    message: Message,
    chime: Option<String>,
    first: bool,
) {
    let mut answering = Box::new(Answering {
        message,
        chime,
        first,
        stage: AnswerStage::Claiming,
        user_id: None,
        owner: None,
        stranger: None,
        began: 0,
        transcript: Vec::new(),
        said: String::new(),
        making_sense: None,
        room: None,
        noted: Pieces::default(),
        sections: Vec::new(),
        reply: String::new(),
        sticker: None,
        deliver: None,
    });
    let owner = match &errand.kind {
        Kind::First { owner, .. } => *owner,
        _ => None,
    };
    let next = if first {
        first_start(cx, &mut answering, owner)
    } else {
        answer_start(cx, &mut answering)
    };
    match next {
        Next::Wait(awaiting) => {
            errand.doing = Doing::Answering(answering);
            park_errand(cx, errand, awaiting);
        }
        Next::Done(replied) => answered_then(cx, errand, replied),
    }
}

/// After one answer: say it again from the newest line if it was held
/// back, then whoever waited.
fn answered_then(cx: &mut Cx, mut errand: Errand, replied: bool) {
    match &mut errand.kind {
        Kind::TakeUp { message, again, .. } => {
            let held = std::mem::take(&mut cx.group.held_back);
            if held
                && *again
                && let Some(newest) = newest_theirs(cx.group, message)
            {
                *again = false;
                *message = newest.clone();
                cx.group.held_back = false;
                start_answer(cx, errand, newest, None, false);
                return;
            }
            *again = true;
        }
        Kind::Look { why, spoke, .. } => {
            if std::mem::take(spoke) && replied && why.is_some_and(|why| why != Why::Answer) {
                spoke_up_now(cx);
            }
        }
        Kind::First { spoke, .. } => {
            if spoke.is_none() {
                *spoke = Some(replied);
                if replied {
                    spoke_up_now(cx);
                }
            }
        }
        Kind::SpokeUp { .. } => return,
    }
    finish(cx, errand, replied);
}

/// End the turn; if a line waited meanwhile, take the turn again for it
/// after the pause.
fn finish(cx: &mut Cx, mut errand: Errand, replied: bool) {
    let now = cx.now();
    end_turn(cx.group, replied, now);
    if cx.group.waiting.is_empty() {
        chain_end(cx, errand);
        return;
    }
    if replied {
        let key = cx.wake(GROUP_PAUSE_SECONDS);
        errand.doing = Doing::Pausing;
        park_errand(cx, errand, key);
        return;
    }
    finish_begin(cx, errand);
}

fn finish_begin(cx: &mut Cx, mut errand: Errand) {
    let now = cx.now();
    match begin(cx.group, now) {
        Ok(()) => {}
        Err(Some(rest)) => {
            let key = cx.wake(rest);
            errand.doing = Doing::Resting;
            park_errand(cx, errand, key);
            return;
        }
        // Someone else took the turn; they will find the line waiting.
        Err(None) => {
            chain_end(cx, errand);
            return;
        }
    }
    let Some(next) = cx.group.waiting.pop_front() else {
        end_turn(cx.group, false, now);
        chain_end(cx, errand);
        return;
    };
    if let Kind::TakeUp { message, .. } = &mut errand.kind {
        *message = next.clone();
        cx.group.held_back = false;
    }
    start_answer(cx, errand, next, None, false);
}

fn chain_end(cx: &mut Cx, mut errand: Errand) {
    match &mut errand.kind {
        Kind::TakeUp { waiter, .. } => {
            let waiter = waiter.clone();
            cx.settle(&waiter, true);
        }
        Kind::Look {
            id, message, why, ..
        } => {
            *message = None;
            *why = None;
            match cx
                .group
                .pending
                .as_ref()
                .map(|line| line.message_id.clone())
            {
                Some(newer) => {
                    *id = newer;
                    look_loop(cx, errand);
                }
                None => {}
            }
        }
        Kind::First { waiter, spoke, .. } => {
            let (waiter, spoke) = (Some(waiter.clone()), spoke.unwrap_or(false));
            cx.settle(&waiter, spoke);
        }
        Kind::SpokeUp { .. } => {}
    }
}

// ---------------------------------------------------------------- glancing

fn look_loop(cx: &mut Cx, mut errand: Errand) {
    let now = cx.now();
    let Kind::Look { id, .. } = &errand.kind else {
        return;
    };
    let Some(pending) = cx.group.pending.as_ref() else {
        return;
    };
    let latest = pending.message_id == *id;
    let overdue = cx
        .group
        .unjudged_since
        .is_some_and(|since| now - since >= seconds(MAX_WAIT_SECONDS));
    if !latest && !overdue {
        return;
    }
    if cx.group.judging || cx.group.busy {
        let key = cx.wake(SETTLE_SECONDS);
        errand.doing = Doing::Settling;
        park_errand(cx, errand, key);
        return;
    }
    cx.group.judging = true;
    cx.group.unjudged_since = None;
    let Some(message) = cx.group.pending.take() else {
        return;
    };
    if let Kind::Look { message: held, .. } = &mut errand.kind {
        *held = Some(message.clone());
    }
    let mut judging = Box::new(Judging {
        message,
        stage: JudgeStage::Identifying,
        owner: None,
    });
    match judge_start(cx, &mut judging) {
        Ok(awaiting) => {
            errand.doing = Doing::Judging(judging);
            park_errand(cx, errand, awaiting);
        }
        Err(decided) => judged(cx, errand, decided),
    }
}

fn judge_start(cx: &mut Cx, judging: &mut Judging) -> Result<String, Option<Decision>> {
    let now = cx.now();
    if stopped_answering(cx.group, &judging.message, now) || muted_now(cx.group, now) {
        return Err(None);
    }
    judging.stage = JudgeStage::Identifying;
    let from = judging.message.from.clone();
    Ok(cx.effect(|id| Effect::Identify { id, from }))
}

/// The pictures in the recent talk she has not seen yet.
fn unseen(group: &Group) -> Vec<Picture> {
    let lines = group.lines.len();
    group
        .lines
        .iter()
        .skip(lines.saturating_sub(CONVERSATION_LINES))
        .filter(|line| !line.hers)
        .flat_map(|line| {
            line.images
                .iter()
                .enumerate()
                .filter(|(index, _)| line.seen.get(*index).is_none_or(Option::is_none))
                .map(|(index, image)| Picture {
                    message_id: line.message_id.clone(),
                    index,
                    image: image.clone(),
                })
                .collect::<Vec<_>>()
        })
        .take(PICTURES_AT_ONCE)
        .collect()
}

/// What she saw, put on the lines; a picture sent there again is the
/// group's.
fn apply_seen(cx: &mut Cx, looked: Vec<Looked>) {
    for looked in looked {
        let key = cx
            .group
            .lines
            .iter()
            .find(|line| !line.hers && line.message_id == looked.message_id)
            .and_then(|line| line.images.get(looked.index))
            .map(|image| image.key.clone());
        if let Some(key) = key
            && cx.group.pictures.get(&key).is_some_and(|sent| *sent >= 2)
        {
            cx.out.effects.push(Effect::PictureAgain {
                key,
                seen: looked.seen.clone(),
            });
        }
        if let Some(line) = cx
            .group
            .lines
            .iter_mut()
            .find(|line| !line.hers && line.message_id == looked.message_id)
        {
            if line.seen.len() <= looked.index {
                line.seen.resize(looked.index + 1, None);
            }
            line.seen[looked.index] = Some(looked.seen);
        }
    }
}

fn judge_step(
    cx: &mut Cx,
    judging: &mut Judging,
    answer: Option<Answer>,
) -> Result<String, Option<Decision>> {
    match (judging.stage, answer) {
        (JudgeStage::Identifying, Some(Answer::Identified { who, owner })) => {
            let owner = match who {
                Who::Member { user_id, bound } => {
                    if !bound {
                        return Err(None);
                    }
                    user_id
                }
                Who::Stranger | Who::Unknown => owner.ok_or(None)?,
            };
            judging.owner = Some(owner);
            let pictures = unseen(cx.group);
            if pictures.is_empty() {
                return Ok(judge_ask(cx, judging));
            }
            judging.stage = JudgeStage::Looking;
            Ok(cx.effect(|id| Effect::Look { id, pictures }))
        }
        (JudgeStage::Looking, Some(Answer::Looked { seen })) => {
            apply_seen(cx, seen);
            Ok(judge_ask(cx, judging))
        }
        (JudgeStage::Judging, Some(Answer::Judged { decision })) => Err(decision),
        _ => Err(None),
    }
}

fn judge_ask(cx: &mut Cx, judging: &mut Judging) -> String {
    let now = cx.now();
    let lines = transcript(cx.group, None, now);
    let conversation = conversation(&lines);
    let late = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| !line.hers)
        .map(|line| age(now, line.at_micros()))
        .filter(|age| *age >= chrono::Duration::minutes(LATE_MINUTES))
        .map(crate::doing::ago_text);
    let last_spoke = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| line.hers)
        .map(|line| crate::doing::ago_text(age(now, line.at_micros())));
    let how_it_went = crate::joining::how_it_went(
        cx.group.spoke_up.len(),
        cx.group.spoke_up.iter().filter(|one| one.taken).count(),
    );
    let share = {
        let recent: Vec<&Line> = cx
            .group
            .lines
            .iter()
            .skip(cx.group.lines.len().saturating_sub(CONVERSATION_LINES))
            .collect();
        let hers = recent.iter().filter(|line| line.hers).count();
        let others = recent
            .iter()
            .filter(|line| !line.hers)
            .map(|line| line.from.as_deref().unwrap_or(&line.name))
            .collect::<std::collections::HashSet<_>>()
            .len();
        crate::joining::share_of_talk(recent.len(), hers, others)
    };
    judging.stage = JudgeStage::Judging;
    let owner = judging.owner.unwrap_or_default();
    cx.effect(|id| Effect::Judge {
        id,
        owner,
        conversation,
        here: Here {
            last_spoke,
            late,
            how_it_went,
            share,
        },
    })
}

fn judged(cx: &mut Cx, mut errand: Errand, decided: Option<Decision>) {
    let now = cx.now();
    cx.group.judging = false;
    // Going on with what she said, without calling her: taken up.
    if decided
        .as_ref()
        .is_some_and(|decided| decided.why == Why::Answer)
    {
        taken_up(cx.group, now);
    }
    let Kind::Look {
        message,
        why,
        spoke,
        ..
    } = &mut errand.kind
    else {
        return;
    };
    let Some(target) = message.clone() else {
        chain_end(cx, errand);
        return;
    };
    match decided {
        // No words, only a reaction on the line.
        Some(Decision {
            why: Why::React,
            reason,
        }) => {
            let to = target.target();
            let emoji = reason.clone();
            let awaiting = cx.effect(|id| Effect::React { id, to, emoji });
            errand.doing = Doing::Reacting { emoji: reason };
            park_errand(cx, errand, awaiting);
        }
        Some(decided) => {
            if begin(cx.group, now).is_ok() {
                *why = Some(decided.why);
                *spoke = true;
                start_answer(cx, errand, target, Some(decided.reason), false);
            } else {
                chain_end(cx, errand);
            }
        }
        None => chain_end(cx, errand),
    }
}

// ---------------------------------------------------------------- answering

fn answer_start(cx: &mut Cx, answering: &mut Answering) -> Next {
    let now = cx.now();
    // Not on this end, she answers nobody there.
    if cx.at.her.away {
        return Next::Done(false);
    }
    if stopped_answering(cx.group, &answering.message, now)
        || read_already(cx.group, &answering.message)
    {
        return Next::Done(false);
    }
    if muted_now(cx.group, now) {
        return Next::Done(false);
    }
    let inbound_id = format!(
        "group:{}:{}",
        answering.message.chat, answering.message.message_id
    );
    answering.stage = AnswerStage::Claiming;
    Next::Wait(cx.effect(|id| Effect::Claim { id, inbound_id }))
}

fn first_start(cx: &mut Cx, answering: &mut Answering, owner: Option<i32>) -> Next {
    let now = cx.now();
    let Some(seen) = cx.group.reach.clone() else {
        return Next::Done(false);
    };
    let Some(owner) = owner else {
        return Next::Done(false);
    };
    answering.owner = Some(owner);
    answering.message = Message {
        message_id: String::new(),
        from: String::new(),
        display_name: String::new(),
        text: crate::sharing::NOBODY_SAID.to_string(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
        ..seen
    };
    let inbound_id = format!(
        "group:{}:first:{}",
        answering.message.chat,
        time(now).timestamp_millis()
    );
    answering.stage = AnswerStage::Claiming;
    Next::Wait(cx.effect(|id| Effect::Claim { id, inbound_id }))
}

fn answer_step(cx: &mut Cx, a: &mut Answering, answer: Option<Answer>) -> Next {
    let now = cx.now();
    match (a.stage, answer) {
        (AnswerStage::Claiming, Some(Answer::Claimed { ok })) => {
            if !ok {
                return Next::Done(false);
            }
            if a.first {
                // Her own turn on the owner's budget: no one's line to read.
                a.began = now;
                a.stage = AnswerStage::Preparing;
                let user_id = a.owner.unwrap_or_default();
                return Next::Wait(cx.effect(|id| Effect::Prepare { id, user_id }));
            }
            let pictures = unseen(cx.group);
            if pictures.is_empty() {
                return identify(cx, a);
            }
            a.stage = AnswerStage::Looking;
            Next::Wait(cx.effect(|id| Effect::Look { id, pictures }))
        }
        (AnswerStage::Looking, Some(Answer::Looked { seen })) => {
            apply_seen(cx, seen);
            identify(cx, a)
        }
        (AnswerStage::Identifying, Some(Answer::Identified { who, owner })) => match who {
            Who::Member { user_id, bound } => {
                if !bound {
                    return Next::Done(false);
                }
                a.user_id = Some(user_id);
                a.owner = Some(user_id);
                a.began = now;
                a.stage = AnswerStage::Preparing;
                Next::Wait(cx.effect(|id| Effect::Prepare { id, user_id }))
            }
            Who::Stranger => {
                let today = cx.at.her.today.clone();
                if !count_today(
                    &mut cx.group.stranger_replies,
                    &today,
                    STRANGER_REPLIES_PER_DAY,
                ) {
                    return Next::Done(false);
                }
                let Some(owner) = owner else {
                    return Next::Done(false);
                };
                a.owner = Some(owner);
                a.stranger = Some(Stranger {
                    who: format!("{}:{}", a.message.platform, a.message.from),
                    name: a.message.display_name.chars().take(40).collect(),
                });
                let read = reading(cx.group, &a.message, now);
                a.transcript = read.transcript;
                a.said = read.said;
                a.room = room(cx.group, now);
                a.noted = noted(cx.group);
                a.stage = AnswerStage::StrangerLedger;
                Next::Wait(cx.effect(|id| Effect::ReadLedger { id }))
            }
            Who::Unknown => Next::Done(false),
        },
        (AnswerStage::Preparing, Some(Answer::Prepared { ok })) => {
            if !ok {
                return Next::Done(false);
            }
            if a.first {
                a.transcript = transcript(cx.group, None, now);
                a.said = said_now(cx.group, &a.message);
                a.room = room(cx.group, now);
                a.noted = noted(cx.group);
                a.stage = AnswerStage::LedgerForTurn;
                return Next::Wait(cx.effect(|id| Effect::ReadLedger { id }));
            }
            let read = reading(cx.group, &a.message, now);
            a.transcript = read.transcript;
            a.said = read.said;
            a.stage = AnswerStage::Sensing;
            let conversation = conversation(&a.transcript);
            let said = a.said.clone();
            Next::Wait(cx.effect(|id| Effect::MakeSense {
                id,
                conversation,
                said,
            }))
        }
        (AnswerStage::Sensing, Some(Answer::Sense { section })) => {
            a.making_sense = section;
            a.room = room(cx.group, now);
            a.noted = noted(cx.group);
            a.stage = AnswerStage::LedgerForTurn;
            Next::Wait(cx.effect(|id| Effect::ReadLedger { id }))
        }
        (AnswerStage::LedgerForTurn, Some(Answer::LedgerRead { stored })) => {
            let differs = differs(cx.group, &a.noted, stored.as_ref());
            let late = if a.first {
                None
            } else {
                seen_late(cx.group, &a.message, now)
            };
            a.stage = AnswerStage::Turning;
            let effect = |id| Effect::Turn {
                id,
                user_id: a.owner.unwrap_or_default(),
                to: a.message.target(),
                input: a.said.clone(),
                transcript: a.transcript.clone(),
                room: a.room.clone(),
                differs,
                making_sense: a.making_sense.clone(),
                late,
                chime: a.chime.clone(),
                speaker: a.message.display_name.clone(),
                first: a.first,
            };
            Next::Wait(cx.effect(effect))
        }
        (
            AnswerStage::Turning,
            Some(Answer::Turned {
                text,
                sticker,
                bound,
            }),
        ) => {
            let text = text.map(|text| text.trim().to_string()).unwrap_or_default();
            if text.is_empty() && sticker.is_none() {
                return Next::Done(false);
            }
            // Unpaired or switched off while she was thinking.
            if !bound {
                return Next::Done(false);
            }
            a.reply = without_reply_mark(&text);
            a.sticker = sticker;
            say(cx, a)
        }
        (AnswerStage::StrangerLedger, Some(Answer::LedgerRead { stored })) => {
            let differs = differs(cx.group, &a.noted, stored.as_ref());
            a.sections = [
                a.room
                    .as_ref()
                    .map(|room| crate::talk_shape::describe(room, "How people type here")),
                differs,
            ]
            .into_iter()
            .flatten()
            .collect();
            a.stage = AnswerStage::StrangerSensing;
            let conversation = conversation(&a.transcript);
            let said = a.said.clone();
            Next::Wait(cx.effect(|id| Effect::MakeSense {
                id,
                conversation,
                said,
            }))
        }
        (AnswerStage::StrangerSensing, Some(Answer::Sense { section })) => {
            a.sections.extend(section);
            a.stage = AnswerStage::StrangerTyping;
            let to = a.message.target();
            Next::Wait(cx.effect(|id| Effect::Typing { id, to }))
        }
        (AnswerStage::StrangerTyping, Some(Answer::Typed)) => {
            a.began = now;
            a.stage = AnswerStage::StrangerTurning;
            let effect = |id| Effect::StrangerTurn {
                id,
                owner: a.owner.unwrap_or_default(),
                stranger: a.stranger.clone().unwrap_or(Stranger {
                    who: String::new(),
                    name: String::new(),
                }),
                transcript: a.transcript.clone(),
                said: a.said.clone(),
                why: a.chime.clone(),
                sections: a.sections.clone(),
            };
            Next::Wait(cx.effect(effect))
        }
        (AnswerStage::StrangerTurning, Some(Answer::Turned { text, sticker, .. })) => {
            let text = text.map(|text| text.trim().to_string()).unwrap_or_default();
            if text.is_empty() && sticker.is_none() {
                return Next::Done(false);
            }
            a.reply = without_reply_mark(&text);
            a.sticker = sticker;
            say(cx, a)
        }
        (AnswerStage::Delivering, answer) => {
            let Some(mut deliver) = a.deliver.take() else {
                return Next::Done(false);
            };
            match deliver_step(cx, &a.message, &mut deliver, answer) {
                Next::Wait(awaiting) => {
                    a.deliver = Some(deliver);
                    Next::Wait(awaiting)
                }
                Next::Done(_) => after_saying(cx, a, deliver.went_out.join("\n")),
            }
        }
        _ => Next::Done(false),
    }
}

fn identify(cx: &mut Cx, a: &mut Answering) -> Next {
    a.stage = AnswerStage::Identifying;
    let from = a.message.from.clone();
    Next::Wait(cx.effect(|id| Effect::Identify { id, from }))
}

/// What was noted for the ledger and not yet written, as she asks for it.
fn noted(group: &Group) -> Pieces {
    Pieces {
        theirs: group.ledger_theirs.clone(),
        hers: group.ledger_hers.clone(),
    }
}

/// Her reply without a （回复 …） mark she copied from the transcript.
pub fn without_reply_mark(reply: &str) -> String {
    let trimmed = reply.trim_start();
    if let Some(rest) = trimmed.strip_prefix("（回复")
        && let Some(end) = rest
            .find('）')
            .filter(|end| rest[..*end].chars().count() <= 200)
    {
        return rest[end + '）'.len_utf8()..].trim_start().to_string();
    }
    reply.to_string()
}

/// Her words, if any, then the sticker she chose.
fn say(cx: &mut Cx, a: &mut Answering) -> Next {
    if a.reply.trim().is_empty() {
        return after_saying(cx, a, String::new());
    }
    let now = cx.now();
    let people = people(cx.group, now);
    let room = room(cx.group, now);
    let lines = myriad_agent_rules::channel::as_messages(&a.reply);
    let most = crate::talk_shape::messages_this_turn(lines.len(), cx.at.rolls.roll());
    let messages = crate::talk_shape::goes_out_as(&lines, most, room.as_ref());
    let said = if a.message.text == crate::sharing::NOBODY_SAID {
        String::new()
    } else {
        a.message.text.clone()
    };
    let mut deliver = Deliver {
        people,
        messages,
        index: 0,
        chunks: Vec::new(),
        chunk: 0,
        sent: false,
        went_out: Vec::new(),
        since: a.began,
        looked: 0,
        said,
        reply: a.reply.clone(),
        owner: a.owner.unwrap_or_default(),
        came: 0,
        left: 0.0,
        stage: DeliverStage::TypingMark,
    };
    a.stage = AnswerStage::Delivering;
    match next_message(cx, &a.message, &mut deliver, a.began) {
        Next::Wait(awaiting) => {
            a.deliver = Some(deliver);
            Next::Wait(awaiting)
        }
        Next::Done(_) => after_saying(cx, a, deliver.went_out.join("\n")),
    }
}

/// What went out of her words: kept as hers; then the sticker.
fn after_saying(cx: &mut Cx, a: &mut Answering, went_out: String) -> Next {
    let now = cx.now();
    let mut sent = false;
    if !a.reply.trim().is_empty() {
        sent = !went_out.is_empty();
        if sent {
            record_hers(cx.group, &went_out, now, cx.out);
        }
    }
    // Held back, the sticker she chose for it is too.
    let held = !a.reply.trim().is_empty() && !sent;
    if let Some(chosen) = a.sticker.take().filter(|_| !held) {
        cx.out.effects.push(Effect::Sticker {
            to: a.message.target(),
            chosen,
        });
        sent = true;
    }
    if sent && !a.first {
        answered(cx.group, &a.message, now);
        if let (Some(stranger), Some(owner)) = (&a.stranger, a.owner) {
            cx.out.effects.push(Effect::StrangerAfter {
                owner,
                stranger: stranger.clone(),
                said: a.said.clone(),
                reply: a.reply.clone(),
                message_id: a.message.message_id.clone(),
            });
        }
    }
    Next::Done(sent)
}

fn text_limit(platform: &str) -> usize {
    match platform {
        "discord" => myriad_agent_rules::channel::DISCORD_TEXT_LIMIT,
        _ => myriad_agent_rules::channel::TELEGRAM_TEXT_LIMIT,
    }
}

/// The next message of her reply: typed, looked over, sent.
fn next_message(cx: &mut Cx, to: &Message, d: &mut Deliver, began: i64) -> Next {
    let now = cx.now();
    if d.index >= d.messages.len() {
        keep_ledger(cx.group, cx.out);
        return Next::Done(true);
    }
    let message = &d.messages[d.index];
    let typing = crate::timing::typing(message.chars().count(), cx.at.rolls.roll());
    // Typing it takes as long as it takes; the first she was already at
    // while she thought.
    let left = if d.index == 0 {
        (typing - (now - began) as f64 / 1_000_000.0).max(0.0)
    } else {
        typing
    };
    if left > 0.0 {
        d.stage = DeliverStage::TypingMark;
        d.left = left;
        let to = to.target();
        return Next::Wait(cx.effect(|id| Effect::Typing { id, to }));
    }
    look_over(cx, to, d)
}

/// Before a message goes: what came meanwhile.
fn look_over(cx: &mut Cx, to: &Message, d: &mut Deliver) -> Next {
    let came = came_since(cx.group, d.since, &to.from);
    if came.len() > d.looked {
        let fresh = &came[d.looked..];
        if d.sent {
            // Under way, the one she answers speaking again, or someone
            // calling her, stops her.
            if fresh.iter().any(|(_, _, stops)| *stops) {
                keep_ledger(cx.group, cx.out);
                return Next::Done(true);
            }
        } else {
            let fresh: Vec<(String, String)> = fresh
                .iter()
                .map(|(who, what, _)| (who.clone(), what.clone()))
                .collect();
            d.came = came.len();
            d.stage = DeliverStage::Fitting;
            let effect = |id| Effect::Ask {
                id,
                voice: "judge".into(),
                owner: d.owner,
                operation: "draft_still".into(),
                timeout_ms: 8000,
                system: crate::draft::system(),
                input: crate::draft::input(&d.said, &d.reply, &fresh),
                schema_name: crate::draft::SCHEMA_NAME.into(),
                schema: crate::draft::schema(),
            };
            return Next::Wait(cx.effect(effect));
        }
        d.looked = came.len();
    }
    chunks(cx, to, d)
}

fn chunks(cx: &mut Cx, to: &Message, d: &mut Deliver) -> Next {
    d.chunks = myriad_agent_rules::channel::split_channel_text(
        &d.messages[d.index],
        text_limit(&to.platform),
    );
    d.chunk = 0;
    send_chunk(cx, to, d)
}

fn send_chunk(cx: &mut Cx, to: &Message, d: &mut Deliver) -> Next {
    if d.chunk >= d.chunks.len() {
        d.index += 1;
        return next_message(cx, to, d, d.since);
    }
    let chunk = &d.chunks[d.chunk];
    let pieces = myriad_agent_rules::mentions::split_mentions(chunk, &d.people)
        .into_iter()
        .map(|piece| match piece {
            myriad_agent_rules::mentions::Piece::Text(text) => Piece::Text { text },
            myriad_agent_rules::mentions::Piece::Mention { id, name } => {
                Piece::Mention { id, name }
            }
        })
        .collect();
    // A line with no message behind it (her saying something first) quotes
    // nothing.
    let quote = !d.sent && !to.message_id.is_empty();
    d.stage = DeliverStage::Sending;
    let target = to.target();
    Next::Wait(cx.effect(|id| Effect::Send {
        id,
        to: target,
        pieces,
        quote,
    }))
}

fn deliver_step(cx: &mut Cx, to: &Message, d: &mut Deliver, answer: Option<Answer>) -> Next {
    let now = cx.now();
    match (d.stage, answer) {
        (DeliverStage::TypingMark, Some(Answer::Typed)) => {
            d.stage = DeliverStage::Typing;
            Next::Wait(cx.wake(d.left))
        }
        (DeliverStage::Typing, None) => look_over(cx, to, d),
        (DeliverStage::Fitting, Some(Answer::Asked { raw })) => {
            let send = raw.as_deref().and_then(crate::draft::parse).unwrap_or(true);
            if !send {
                cx.group.held_back = true;
                keep_ledger(cx.group, cx.out);
                return Next::Done(true);
            }
            d.looked = d.came;
            chunks(cx, to, d)
        }
        (DeliverStage::Sending, Some(Answer::Sent { ok })) => {
            if !ok {
                keep_ledger(cx.group, cx.out);
                return Next::Done(false);
            }
            let chunk = d.chunks[d.chunk].clone();
            // How long whoever called her waited for her first words.
            let waited = (!d.sent && to.addressed)
                .then(|| said_ago(cx.group, to, now))
                .flatten()
                .map(|age| age.num_milliseconds() as f64 / 1000.0);
            let typed = crate::talk_shape::typed_by(HER, time(now).timestamp(), &chunk);
            note_said(cx.group, typed, waited, Some(&chunk));
            d.sent = true;
            d.went_out.push(chunk);
            d.chunk += 1;
            send_chunk(cx, to, d)
        }
        _ => {
            keep_ledger(cx.group, cx.out);
            Next::Done(false)
        }
    }
}

// ---------------------------------------------------------------- first

fn speak_first(cx: &mut Cx, reason: String, owner: Option<i32>, waiter: String) {
    let now = cx.now();
    if begin(cx.group, now).is_err() {
        cx.settle(&Some(waiter), false);
        return;
    }
    let id = cx.id();
    let errand = Errand {
        id,
        awaiting: String::new(),
        kind: Kind::First {
            waiter,
            reason: reason.clone(),
            owner,
            spoke: None,
        },
        doing: Doing::Timing,
    };
    let message = cx.group.reach.clone().unwrap_or(Message {
        platform: String::new(),
        chat: String::new(),
        message_id: String::new(),
        thread: None,
        from: String::new(),
        display_name: String::new(),
        text: String::new(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
    });
    start_answer(cx, errand, message, Some(reason), true);
}

pub(super) fn offer(group: &Group, now: i64) -> Option<Offer> {
    if group.busy || in_talk(group, now) || muted_now(group, now) {
        return None;
    }
    group.reach.as_ref()?;
    let quiet = group
        .lines
        .back()
        .map(|line| line.at_micros())
        .or(group.seen_before)
        .map(|at| now - at);
    if quiet.is_some_and(|quiet| quiet > SEEN_WITHIN_SECONDS * 1_000_000) {
        return None;
    }
    let lines: Vec<String> = group
        .lines
        .iter()
        .rev()
        .take(SHARE_LINES)
        .rev()
        .map(|line| {
            if line.hers {
                format!("you：{}", line.text)
            } else {
                format!("{}：{}", line.name, line.said())
            }
        })
        .collect();
    let quiet_for = match quiet {
        Some(quiet) => crate::doing::ago_text(chrono::Duration::microseconds(quiet)),
        None => "a good while (nothing said there lately)".to_string(),
    };
    let spoke_up_lately = group
        .spoke_up
        .iter()
        .map(|one| {
            serde_json::json!({
                "ago": crate::doing::ago_text(chrono::Duration::microseconds((now - one.at).max(0))),
                "takenUp": one.taken,
            })
        })
        .collect();
    Some(Offer {
        lines,
        quiet_for,
        spoke_up_lately,
    })
}
