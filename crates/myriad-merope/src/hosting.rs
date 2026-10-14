//! A game she hosts at a table (D2c): one shape for every kind, the rules of
//! each plugged in.
//!
//! The rules are mechanical and deterministic: who sits where, what each seat
//! is dealt (by the rolls the end hands in), what may be done now, how a move
//! changes the game, when it is over. No rule asks a model. A model is asked
//! only where meaning is needed: whether a question fits a story's truth, what
//! she says in her own voice, which of the moves open to her seat she takes
//! when she plays one. Those asks are given views, never the game itself.
//!
//! Every fact a game holds says whom it is for: everyone at the table, one
//! seat, or the host. What anyone is shown is made from the facts for them
//! and nothing else: a [`SeatView`] has the facts for all and for that seat;
//! a [`PublicView`] the facts for all; a [`HostView`] everything, only for a
//! game she hosts herself. Views are made here only, from a game's facts; a
//! prompt for her as a player or as the voice of a game the core referees
//! takes a view, so what she does not know as a seat cannot reach her.
//!
//! ```compile_fail
//! // A view cannot be made by hand, with facts of anyone's choosing.
//! let view = myriad_merope::hosting::SeatView { seat: 0, facts: Vec::new(), choices: Vec::new() };
//! ```

use serde::{Deserialize, Serialize};

use crate::group::Rolls;

/// Whom a fact of a game is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "to", content = "seat")]
pub enum Audience {
    All,
    Seat(usize),
    Host,
}

/// One thing a game holds, and whom it is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub to: Audience,
    pub text: String,
}

impl Fact {
    pub fn all(text: impl Into<String>) -> Self {
        Self {
            to: Audience::All,
            text: text.into(),
        }
    }

    pub fn seat(seat: usize, text: impl Into<String>) -> Self {
        Self {
            to: Audience::Seat(seat),
            text: text.into(),
        }
    }

    pub fn host(text: impl Into<String>) -> Self {
        Self {
            to: Audience::Host,
            text: text.into(),
        }
    }
}

/// Someone at the table, and whether it is her.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seat {
    /// Who they are on the platform (`telegram:123`).
    pub who: String,
    pub name: String,
    #[serde(default)]
    pub hers: bool,
}

/// The rules of one kind of game.
pub trait Rules {
    type State;
    /// What the game is called where it is kept (`soup`).
    const KIND: &'static str;
    /// Who sits at it; empty when anyone at the table may play (a story the
    /// table guesses together).
    fn seats(state: &Self::State) -> Vec<Seat>;
    /// Everything it holds, each with whom it is for.
    fn facts(state: &Self::State) -> Vec<Fact>;
    /// What `seat` may do now, as plain choices.
    fn choices(state: &Self::State, seat: usize) -> Vec<String>;
    /// `seat` takes choice `index` (one of [`Rules::choices`]); chance from
    /// `rolls`. A choice that is not open does nothing.
    fn take(state: &mut Self::State, seat: usize, index: usize, rolls: &mut Rolls);
    fn over(state: &Self::State) -> bool;
}

/// What one seat knows, and may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatView {
    seat: usize,
    facts: Vec<String>,
    choices: Vec<String>,
}

impl SeatView {
    pub fn seat(&self) -> usize {
        self.seat
    }

    pub fn facts(&self) -> &[String] {
        &self.facts
    }

    pub fn choices(&self) -> &[String] {
        &self.choices
    }
}

/// What everyone at the table knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicView {
    facts: Vec<String>,
}

impl PublicView {
    pub fn facts(&self) -> &[String] {
        &self.facts
    }
}

/// Everything, for a game she hosts herself (she is no seat in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostView {
    facts: Vec<String>,
}

impl HostView {
    pub fn facts(&self) -> &[String] {
        &self.facts
    }
}

/// What `seat` knows: the facts for all, and for that seat.
pub fn seat_view<R: Rules>(state: &R::State, seat: usize) -> SeatView {
    SeatView {
        seat,
        facts: R::facts(state)
            .into_iter()
            .filter(|fact| matches!(fact.to, Audience::All) || fact.to == Audience::Seat(seat))
            .map(|fact| fact.text)
            .collect(),
        choices: R::choices(state, seat),
    }
}

/// What everyone at the table knows.
pub fn public_view<R: Rules>(state: &R::State) -> PublicView {
    PublicView {
        facts: R::facts(state)
            .into_iter()
            .filter(|fact| matches!(fact.to, Audience::All))
            .map(|fact| fact.text)
            .collect(),
    }
}

/// Everything, when she hosts it and holds no seat; none when she does.
pub fn host_view<R: Rules>(state: &R::State) -> Option<HostView> {
    if R::seats(state).iter().any(|seat| seat.hers) {
        return None;
    }
    Some(HostView {
        facts: R::facts(state).into_iter().map(|fact| fact.text).collect(),
    })
}

/// What she is asked as a player of `game` (named, with its rules told in
/// `rules`): from her seat's view alone, the choices numbered.
pub fn player_prompt(soul: &str, game: &str, rules: &str, view: &SeatView) -> (String, String) {
    let system = format!(
        "{soul}\n\nYou are playing {game} as one of the players. {rules} You know only what is shown to you here; whatever others were dealt or did in secret, you do not know. Pick one of the choices by its number, as yourself."
    );
    let input = serde_json::json!({
        "youKnow": view.facts,
        "choices": view
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| serde_json::json!({ "n": index, "choice": choice }))
            .collect::<Vec<_>>(),
    })
    .to_string();
    (system, input)
}

/// What she says, in her own voice, for a game the core referees: from what
/// everyone knows, and what just happened in it.
pub fn narration_prompt(
    soul: &str,
    game: &str,
    view: &PublicView,
    latest: &[String],
) -> (String, String) {
    let system = format!(
        "{soul}\n\nYou are the voice of a game of {game} at this table: the rules are kept for you; say what just happened and what comes next, as yourself, in a line or two. Say only what everyone may know."
    );
    let input = serde_json::json!({ "everyoneKnows": view.facts, "justNow": latest }).to_string();
    (system, input)
}

/// The choice she made as a player, read back: one of her seat's choices,
/// or none.
pub fn read_choice(raw: &str, view: &SeatView) -> Option<usize> {
    #[derive(Deserialize)]
    struct Chosen {
        n: usize,
    }
    let chosen: Chosen = crate::answer::parse(raw)?;
    (chosen.n < view.choices.len()).then_some(chosen.n)
}

/// Turtle soup, as a game at a table: anyone there asks, she alone knows
/// the truth.
pub struct Soup;

impl Rules for Soup {
    type State = crate::soup::Game;
    const KIND: &'static str = "soup";

    fn seats(_: &Self::State) -> Vec<Seat> {
        Vec::new()
    }

    fn facts(game: &Self::State) -> Vec<Fact> {
        let mut facts = vec![
            Fact::all(format!("surface: {}", game.surface)),
            Fact::host(format!("truth: {}", game.truth)),
        ];
        facts.extend(
            game.keys
                .iter()
                .map(|key| Fact::host(format!("key: {key}"))),
        );
        facts.extend(game.asked.iter().map(|asked| {
            Fact::all(format!(
                "asked: {} — {}",
                asked.question,
                asked.verdict.says()
            ))
        }));
        facts
    }

    fn choices(_: &Self::State, _: usize) -> Vec<String> {
        Vec::new()
    }

    fn take(_: &mut Self::State, _: usize, _: usize, _: &mut Rolls) {}

    fn over(game: &Self::State) -> bool {
        game.ending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test-only game with private seats: each seat is dealt a secret mark;
    /// a turn passes or guesses the next seat's mark; three rounds and it is
    /// over. It holds what a real party game does (secrets per seat, a public
    /// log, choices) and nothing else.
    #[derive(Debug, Clone)]
    struct Marks {
        seats: Vec<Seat>,
        marks: Vec<String>,
        log: Vec<String>,
        turn: usize,
        rounds: usize,
    }

    impl Marks {
        fn deal(players: usize, hers: usize, rolls: &mut Rolls) -> Self {
            let seats = (0..players)
                .map(|seat| Seat {
                    who: format!("telegram:{seat}"),
                    name: format!("玩家{seat}"),
                    hers: seat == hers,
                })
                .collect();
            let marks = (0..players)
                .map(|seat| format!("SECRET-{seat}-{}", rolls.range(1000..=9999)))
                .collect();
            Self {
                seats,
                marks,
                log: Vec::new(),
                turn: 0,
                rounds: 0,
            }
        }
    }

    impl Rules for Marks {
        type State = Marks;
        const KIND: &'static str = "marks";

        fn seats(game: &Marks) -> Vec<Seat> {
            game.seats.clone()
        }

        fn facts(game: &Marks) -> Vec<Fact> {
            let mut facts: Vec<Fact> = game
                .marks
                .iter()
                .enumerate()
                .map(|(seat, mark)| Fact::seat(seat, format!("your mark: {mark}")))
                .collect();
            facts.extend(game.log.iter().map(Fact::all));
            facts
        }

        fn choices(game: &Marks, seat: usize) -> Vec<String> {
            if seat != game.turn || Self::over(game) {
                return Vec::new();
            }
            vec!["pass".into(), "guess the next seat's mark".into()]
        }

        fn take(game: &mut Marks, seat: usize, index: usize, rolls: &mut Rolls) {
            if Self::choices(game, seat).get(index).is_none() {
                return;
            }
            let next = (seat + 1) % game.seats.len();
            // A guess is a number drawn; whether it hit is told to all, the
            // mark itself never.
            let line = if index == 0 {
                format!("{} passed", game.seats[seat].name)
            } else {
                let guess = format!("SECRET-{next}-{}", rolls.range(1000..=9999));
                let hit = guess == game.marks[next];
                format!(
                    "{} guessed {}'s mark: {}",
                    game.seats[seat].name,
                    game.seats[next].name,
                    if hit { "right" } else { "wrong" }
                )
            };
            game.log.push(line);
            game.turn = next;
            if next == 0 {
                game.rounds += 1;
            }
        }

        fn over(game: &Marks) -> bool {
            game.rounds >= 3
        }
    }

    /// Whatever the seed, the number of players and which seat is hers, at
    /// every step: what she is asked as her seat, and as the voice of the
    /// game, holds no other seat's mark; her own is in what her seat knows.
    #[test]
    fn what_a_seat_does_not_know_never_reaches_her() {
        for seed in 0..300u64 {
            let mut rolls = Rolls::new(seed, Vec::new());
            let players = 3 + rolls.range(0..=5) as usize;
            let hers = rolls.range(0..=(players as u64 - 1)) as usize;
            let mut game = Marks::deal(players, hers, &mut rolls);
            let mut steps = 0;
            while !Marks::over(&game) && steps < 100 {
                let seat = game.turn;
                let mine = seat_view::<Marks>(&game, hers);
                let (system, input) = player_prompt("她", "marks", "", &mine);
                let (voice_system, voice_input) =
                    narration_prompt("她", "marks", &public_view::<Marks>(&game), &game.log);
                let told = [system, input, voice_system, voice_input].join("\n");
                for (other, mark) in game.marks.iter().enumerate() {
                    if other == hers {
                        continue;
                    }
                    assert!(
                        !told.contains(mark),
                        "seed {seed}: seat {other}'s mark reached her (seat {hers})"
                    );
                }
                assert!(
                    mine.facts()
                        .iter()
                        .any(|fact| fact.contains(&game.marks[hers]))
                );
                // The voice of the game knows no seat's mark, hers included.
                let voice = public_view::<Marks>(&game).facts().join("\n");
                assert!(game.marks.iter().all(|mark| !voice.contains(mark)));
                let open = Marks::choices(&game, seat).len();
                let index = rolls.range(0..=(open as u64 - 1)) as usize;
                Marks::take(&mut game, seat, index, &mut rolls);
                steps += 1;
            }
            assert!(Marks::over(&game), "seed {seed}");
        }
    }

    /// She holds a seat: the game has no host's view of it for her.
    #[test]
    fn she_who_plays_does_not_host() {
        let mut rolls = Rolls::new(7, Vec::new());
        let game = Marks::deal(4, 2, &mut rolls);
        assert!(host_view::<Marks>(&game).is_none());
        let mut theirs = game.clone();
        for seat in &mut theirs.seats {
            seat.hers = false;
        }
        let host = host_view::<Marks>(&theirs).unwrap();
        assert!(
            theirs
                .marks
                .iter()
                .all(|mark| host.facts().iter().any(|fact| fact.contains(mark)))
        );
    }

    /// What she picks as a player is one of her seat's choices, or nothing.
    #[test]
    fn her_choice_is_one_of_her_seats() {
        let mut rolls = Rolls::new(3, Vec::new());
        let game = Marks::deal(3, 0, &mut rolls);
        let mine = seat_view::<Marks>(&game, 0);
        assert_eq!(read_choice(r#"{"n":1}"#, &mine), Some(1));
        assert_eq!(read_choice(r#"{"n":5}"#, &mine), None);
        let theirs = seat_view::<Marks>(&game, 1);
        assert_eq!(read_choice(r#"{"n":0}"#, &theirs), None);
    }

    /// In a soup she hosts, the truth is hers and the table's view lacks it.
    #[test]
    fn a_soups_truth_is_the_hosts() {
        let game = crate::soup::Game {
            surface: "他喝了一口海龟汤".into(),
            truth: "他曾吃过人肉".into(),
            keys: vec!["吃过人肉".into()],
            asked: Vec::new(),
            found: Vec::new(),
            ending: None,
            solver: None,
            started: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            last: None,
            made: None,
        };
        let table = public_view::<Soup>(&game).facts().join("\n");
        assert!(table.contains("他喝了一口海龟汤") && !table.contains("人肉"));
        let host = host_view::<Soup>(&game).unwrap().facts().join("\n");
        assert!(host.contains("他曾吃过人肉"));
    }
}
