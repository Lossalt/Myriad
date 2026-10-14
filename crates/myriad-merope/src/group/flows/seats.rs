//! A game with seats at the group's table (D2c), the core its referee: who
//! joins in the group while sign-up is open, whether each can be whispered
//! to (those who cannot are told in the group how to open it, by platform),
//! what each seat is dealt and when it is their move, whispered to them
//! alone; their moves come back in private, are checked against what their
//! seat may do, and what everyone may know of them is told in the group. A
//! seat that does not move in time passes. Over, the group remembers it.
//!
//! Only what a seat may know is whispered to it (`crate::hosting`'s views);
//! the group hears only what everyone may know.

use serde::{Deserialize, Serialize};

use super::*;
use crate::hosting::{Seat, SeatGame};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeatStage {
    SignUp,
    Checking,
    Playing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seating {
    pub kind: String,
    pub name: String,
    pub stage: SeatStage,
    /// Where the group hears it.
    pub chat: Target,
    #[serde(default)]
    pub joined: Vec<Seat>,
    /// Which of those who joined is being whispered to.
    #[serde(default)]
    pub checking: usize,
    #[serde(default)]
    pub reachable: Vec<Seat>,
    #[serde(default)]
    pub game: Option<SeatGame>,
    /// The timer that ends sign-up, or a seat's time for its move.
    #[serde(default)]
    pub timer: Option<String>,
}

fn platform_of(who: &str) -> (&str, &str) {
    who.split_once(':').unwrap_or(("", who))
}

/// Say `text` in the group, as hers.
fn tell(cx: &mut Cx, s: &Seating, text: &str) {
    let now = cx.now();
    record_hers(cx.group, text, now, cx.out);
    let (to, pieces) = (
        s.chat.clone(),
        vec![Piece::Text {
            text: text.to_string(),
        }],
    );
    cx.effect(|id| Effect::Send {
        id,
        to,
        pieces,
        quote: false,
    });
}

/// Whisper `text` to `seat`; the id it is answered under.
fn whisper(cx: &mut Cx, s: &Seating, seat: &Seat, text: String) -> String {
    let (platform, user) = platform_of(&seat.who);
    let (platform, user, chat) = (platform.to_string(), user.to_string(), s.chat.chat.clone());
    cx.effect(|id| Effect::Whisper {
        id,
        platform,
        user,
        chat,
        text,
    })
}

/// Wait `after` seconds for the game at the table.
fn wait(cx: &mut Cx, after: f64) {
    let key = cx.wake(after);
    let errand = cx.id();
    if let Some(s) = cx.group.seating.as_mut() {
        s.timer = Some(key.clone());
    }
    cx.group.errands.push(Errand {
        id: errand,
        awaiting: key,
        kind: Kind::Seats,
        doing: Doing::Seats,
    });
}

/// Her words named a seat game of `kind`: sign-up opens at the table `chat`.
/// The line she adds to her words, or none when a game is on there already
/// (one game a table) or there is no such game.
pub(crate) fn open_sign_up(cx: &mut Cx, kind: &str, chat: Target) -> Option<String> {
    if cx.group.seating.is_some() {
        return None;
    }
    let known = crate::hosting::seat_kind(kind)?;
    cx.group.seating = Some(Seating {
        kind: known.kind.to_string(),
        name: known.name.to_string(),
        stage: SeatStage::SignUp,
        chat: Target {
            message_id: String::new(),
            ..chat
        },
        joined: Vec::new(),
        checking: 0,
        reachable: Vec::new(),
        game: None,
        timer: None,
    });
    wait(cx, crate::hosting::SIGN_UP_SECONDS);
    Some(crate::hosting::sign_up_line(known.name))
}

/// A line of the group's: while sign-up is open, one who says they join is
/// seated (up to the game's most).
pub(crate) fn heard_at_table(cx: &mut Cx, message: &Message) {
    let Some(s) = cx.group.seating.as_mut() else {
        return;
    };
    if s.stage != SeatStage::SignUp || !crate::hosting::joins(&message.text) {
        return;
    }
    let who = format!("{}:{}", message.platform, message.from);
    let most = crate::hosting::seat_kind(&s.kind).map_or(0, |kind| kind.most);
    if s.joined.iter().any(|seat| seat.who == who) || s.joined.len() >= most {
        return;
    }
    s.joined.push(Seat {
        who,
        name: message.display_name.clone(),
        hers: false,
    });
}

/// The errand of the table: a timer came, or what a whisper came to.
pub(crate) fn seats_step(cx: &mut Cx, errand: Errand, answer: Option<Answer>) {
    let Some(mut s) = cx.group.seating.take() else {
        return;
    };
    match (s.stage, answer) {
        // Sign-up closes: each who joined is whispered to, one by one.
        (SeatStage::SignUp, None) if s.timer.as_deref() == Some(errand.awaiting.as_str()) => {
            s.stage = SeatStage::Checking;
            s.checking = 0;
            check_next(cx, s, errand);
        }
        (SeatStage::Checking, Some(Answer::Whispered { ok })) => {
            let seat = s.joined[s.checking].clone();
            if ok {
                cx.out.effects.push(Effect::Seat {
                    who: seat.who.clone(),
                });
                s.reachable.push(seat);
            } else {
                let (platform, _) = platform_of(&seat.who);
                let line = crate::hosting::unreachable_line(platform, &seat.name);
                tell(cx, &s, &line);
            }
            s.checking += 1;
            check_next(cx, s, errand);
        }
        // A seat's time ran out: it passes.
        (SeatStage::Playing, None) if s.timer.as_deref() == Some(errand.awaiting.as_str()) => {
            let Some(seat) = moving(&s) else {
                cx.group.seating = Some(s);
                return;
            };
            let name = s.reachable[seat].name.clone();
            tell(cx, &s, &crate::hosting::no_move_line(&name));
            moved(cx, s, seat, 0);
        }
        // A stale timer, or nothing it waits for.
        _ => cx.group.seating = Some(s),
    }
}

/// Whisper the next who joined; all whispered, the game deals or is called
/// off.
fn check_next(cx: &mut Cx, s: Seating, mut errand: Errand) {
    if let Some(seat) = s.joined.get(s.checking).cloned() {
        let awaiting = whisper(cx, &s, &seat, crate::hosting::seated_line(&s.name));
        cx.group.seating = Some(s);
        errand.awaiting = awaiting;
        errand.doing = Doing::Seats;
        cx.group.errands.push(errand);
        return;
    }
    let fewest = crate::hosting::seat_kind(&s.kind).map_or(usize::MAX, |kind| kind.fewest);
    if s.reachable.len() < fewest {
        tell(cx, &s, &crate::hosting::too_few_line(&s.name, fewest));
        for seat in &s.reachable {
            cx.out.effects.push(Effect::Unseat {
                who: seat.who.clone(),
            });
        }
        return;
    }
    let Some(game) = SeatGame::deal(&s.kind, s.reachable.clone(), &mut cx.at.rolls) else {
        return;
    };
    let mut s = s;
    s.stage = SeatStage::Playing;
    tell(cx, &s, &crate::hosting::started_line(&s.name, &s.reachable));
    // What each seat is dealt, to it alone.
    for (index, seat) in s.reachable.clone().iter().enumerate() {
        let view = game.seat_view(index);
        whisper(cx, &s, seat, view.facts().join("\n"));
    }
    s.game = Some(game);
    next_move(cx, s);
}

/// The seat whose move it is, if any.
fn moving(s: &Seating) -> Option<usize> {
    let game = s.game.as_ref()?;
    (0..s.reachable.len()).find(|seat| !game.seat_view(*seat).choices().is_empty())
}

/// Whisper the seat whose move it is what it may do, and give it time.
fn next_move(cx: &mut Cx, s: Seating) {
    let Some(seat) = moving(&s) else {
        return over(cx, s);
    };
    let Some(view) = s.game.as_ref().map(|game| game.seat_view(seat)) else {
        return;
    };
    let who = s.reachable[seat].clone();
    whisper(cx, &s, &who, crate::hosting::your_move_line(&view));
    cx.group.seating = Some(s);
    wait(cx, crate::hosting::MOVE_SECONDS);
}

/// `seat` takes choice `index`: what everyone may know of it is told in the
/// group, and the game goes on.
fn moved(cx: &mut Cx, mut s: Seating, seat: usize, index: usize) {
    let Some(mut game) = s.game.take() else {
        return;
    };
    let before = game.public_view().facts().len();
    game.take(seat, index, &mut cx.at.rolls);
    let after = game.public_view();
    let news = after.facts()[before.min(after.facts().len())..].join("\n");
    s.game = Some(game);
    if !news.is_empty() {
        tell(cx, &s, &news);
    }
    if s.game.as_ref().is_some_and(SeatGame::over) {
        return over(cx, s);
    }
    next_move(cx, s);
}

/// Over: the group remembers it, and nobody sits at it any more.
fn over(cx: &mut Cx, s: Seating) {
    tell(cx, &s, &crate::hosting::over_line(&s.name));
    cx.out.effects.push(Effect::Write {
        of: Of::Others,
        what: ToWrite::RememberInVenue {
            venue: super::sense::stored(&format!("{}:{}", s.chat.platform, s.chat.chat)),
            content: crate::hosting::remembered_in_group(&s.name, &s.reachable),
            evidence: serde_json::json!({ "game": s.kind }).to_string(),
            source: "game".into(),
        },
    });
    for seat in &s.reachable {
        cx.out.effects.push(Effect::Unseat {
            who: seat.who.clone(),
        });
    }
}

/// A seat whispered back: a move when it is theirs and one of what their
/// seat may do (taken), else not the game's.
pub(crate) fn whispered(cx: &mut Cx, who: &str, text: &str) -> bool {
    let Some(s) = cx.group.seating.take() else {
        return false;
    };
    let seat = s.reachable.iter().position(|seat| seat.who == who);
    let chosen = match (s.stage, seat, s.game.as_ref()) {
        (SeatStage::Playing, Some(seat), Some(game)) => {
            crate::hosting::read_move(text, &game.seat_view(seat)).map(|index| (seat, index))
        }
        _ => None,
    };
    let Some((seat, index)) = chosen else {
        cx.group.seating = Some(s);
        return false;
    };
    moved(cx, s, seat, index);
    true
}
