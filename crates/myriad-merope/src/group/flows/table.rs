//! A game at the group's table, as her group mind runs it (D2c): the one on
//! now read back, a line judged in it, one started when she says she will
//! host one, and one over or left for hours put away and remembered. Turtle
//! soup is the kind there is (see `crate::hosting::Soup`); the game is kept
//! by the end, as the group's.

use serde::{Deserialize, Serialize};

use super::*;

/// A judgment of a line in the game, within this long.
const JUDGE_WITHIN_MS: u64 = 30_000;
/// Making one up: a first try that thinks freely, then one more that thinks
/// little.
const FIRST_TRY_MS: u64 = 45_000;
const SECOND_TRY_MS: u64 = 30_000;
/// Her own puzzles read back.
const MADE_READ: u64 = 40;
const MADE_SOURCE: &str = "made_soup";

/// What she does at the table.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AtTableDo {
    /// Their line, in the game on now: the section she answers from.
    Turn,
    /// She said she would host one: how it opens.
    Start,
    /// After her answer: a game it ended is over.
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AtTableStage {
    Reading,
    Judging,
    Hers,
    Soul,
    Recent,
    Making,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtTable {
    pub what: AtTableDo,
    /// The group, as sessions keep it.
    pub venue: String,
    pub asker: String,
    pub words: String,
    /// Who judging and making are billed to.
    pub owner: i32,
    /// A member's turn it runs within, when it is one.
    #[serde(default)]
    pub turn: Option<String>,
    pub stage: AtTableStage,
    #[serde(default)]
    pub game: Option<crate::soup::Game>,
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub soul: String,
    #[serde(default)]
    pub tries: u8,
}

/// What a step at the table came to: waiting, or its text (the section, how
/// it opens; none after).
pub(crate) enum AtTableNext {
    Wait(String),
    Done(Option<String>),
}

/// Where the group's game is kept.
fn table_id(venue: &str) -> String {
    crate::soup::Table::Group(venue.to_string()).record_id()
}

/// Begin `what` at the group `venue`'s table.
pub(crate) fn at_table(
    cx: &mut Cx,
    what: AtTableDo,
    venue: &str,
    asker: &str,
    words: &str,
    owner: i32,
    turn: Option<String>,
) -> (AtTable, AtTableNext) {
    let mut t = AtTable {
        what,
        venue: venue.to_string(),
        asker: asker.to_string(),
        words: words.to_string(),
        owner,
        turn,
        stage: AtTableStage::Reading,
        game: None,
        input: String::new(),
        soul: String::new(),
        tries: 0,
    };
    let next = match what {
        // Hers to bring out first: a puzzle of her own this table has not
        // played.
        AtTableDo::Start => {
            t.stage = AtTableStage::Hers;
            let what = ToRead::OwnRows {
                source: MADE_SOURCE.into(),
                limit: MADE_READ,
            };
            AtTableNext::Wait(cx.effect(|id| Effect::Read {
                id,
                of: Of::Hers,
                what,
            }))
        }
        _ => {
            let table = table_id(venue);
            AtTableNext::Wait(cx.effect(|id| Effect::Read {
                id,
                of: Of::Others,
                what: ToRead::Game { table },
            }))
        }
    };
    (t, next)
}

/// Put a game away: taken off the table, how it went kept with her puzzle
/// if it is one of hers, and the group remembering it.
fn put_away(cx: &mut Cx, t: &AtTable, game: &crate::soup::Game) {
    let table = table_id(&t.venue);
    cx.out.effects.push(Effect::Write {
        of: Of::Others,
        what: ToWrite::TakeGame {
            table: table.clone(),
        },
    });
    if let Some(id) = &game.made {
        let tried = crate::soup::tried(&table, game, lines::time(cx.now()));
        cx.out.effects.push(Effect::Write {
            of: Of::Hers,
            what: ToWrite::Tried {
                id: id.clone(),
                tried,
            },
        });
    }
    cx.out.effects.push(Effect::Write {
        of: Of::Others,
        what: ToWrite::RememberInVenue {
            venue: super::sense::stored(&t.venue),
            content: crate::soup::remembered_in_group(game),
            evidence: serde_json::json!({ "game": "soup" }).to_string(),
            source: "game".into(),
        },
    });
}

/// Keep the game on the table.
fn keep(cx: &mut Cx, t: &AtTable, game: &crate::soup::Game) {
    let table = table_id(&t.venue);
    let game = serde_json::to_value(game).unwrap_or_default();
    cx.out.effects.push(Effect::Write {
        of: Of::Others,
        what: ToWrite::Game { table, game },
    });
}

#[allow(clippy::too_many_arguments)]
fn ask(
    cx: &mut Cx,
    t: &AtTable,
    voice: &str,
    operation: &str,
    within: u64,
    system: String,
    input: String,
    schema_name: &str,
    schema: Value,
) -> String {
    let (owner, turn) = (t.owner, t.turn.clone());
    let voice = voice.to_string();
    let operation = operation.to_string();
    let schema_name = schema_name.to_string();
    cx.effect(|id| Effect::Ask {
        id,
        how: AskKind::Json,
        voice,
        owner,
        operation,
        timeout_ms: within,
        system,
        input,
        schema_name,
        schema,
        turn,
    })
}

/// Make one up: the first try or the second.
fn make(cx: &mut Cx, t: &mut AtTable) -> AtTableNext {
    t.stage = AtTableStage::Making;
    t.tries += 1;
    let (voice, within) = if t.tries == 1 {
        ("hersAtLength", FIRST_TRY_MS)
    } else {
        ("hers", SECOND_TRY_MS)
    };
    let system = crate::soup::start_system(&t.soul);
    let input = t.input.clone();
    AtTableNext::Wait(ask(
        cx,
        t,
        voice,
        "soup_start",
        within,
        system,
        input,
        crate::soup::START_SCHEMA,
        crate::soup::start_schema(),
    ))
}

/// A step at the table.
pub(crate) fn at_table_step(cx: &mut Cx, t: &mut AtTable, answer: Option<Answer>) -> AtTableNext {
    let now = cx.now();
    match (t.stage, answer) {
        (AtTableStage::Reading, Some(Answer::Game { game })) => {
            let Some(game) =
                game.and_then(|game| serde_json::from_value::<crate::soup::Game>(game).ok())
            else {
                return AtTableNext::Done(None);
            };
            // Left unfinished long enough: put away, not still on.
            if game.left(lines::time(now)) {
                put_away(cx, t, &game);
                return AtTableNext::Done(None);
            }
            match t.what {
                AtTableDo::After => {
                    if game.ending.is_some() {
                        put_away(cx, t, &game);
                    }
                    AtTableNext::Done(None)
                }
                _ => {
                    t.stage = AtTableStage::Judging;
                    let system = crate::soup::JUDGE_SYSTEM.to_string();
                    let input = crate::soup::judge_input(&game, &t.words);
                    let schema = crate::soup::judge_schema(game.keys.len());
                    t.game = Some(game);
                    AtTableNext::Wait(ask(
                        cx,
                        t,
                        "judge",
                        "soup_judge",
                        JUDGE_WITHIN_MS,
                        system,
                        input,
                        crate::soup::JUDGE_SCHEMA,
                        schema,
                    ))
                }
            }
        }
        (AtTableStage::Judging, Some(Answer::Asked { raw, .. })) => {
            let Some(mut game) = t.game.take() else {
                return AtTableNext::Done(None);
            };
            let asker = Some(t.asker.as_str());
            let judged = raw
                .as_deref()
                .and_then(crate::answer::parse::<crate::soup::Judged>);
            let Some(judged) = judged else {
                // Unjudged, she must not guess an answer.
                return AtTableNext::Done(Some(crate::soup::section(&game, None, true, asker)));
            };
            crate::soup::apply(&mut game, &judged, asker, &t.words);
            game.last = Some(lines::time(now));
            keep(cx, t, &game);
            AtTableNext::Done(Some(crate::soup::section(
                &game,
                Some(judged.verdict),
                true,
                asker,
            )))
        }
        (AtTableStage::Hers, Some(Answer::Rows { rows })) => {
            // Hers, oldest first, as kept with each.
            let mut made: Vec<crate::making::Made> = rows
                .iter()
                .filter_map(|row| {
                    let mut made: crate::making::Made =
                        serde_json::from_str(row.evidence.as_deref()?).ok()?;
                    made.id = row.id.clone();
                    Some(made)
                })
                .collect();
            made.reverse();
            let table = table_id(&t.venue);
            let mine = crate::making::named_in(&made, &t.words)
                .filter(|made| !made.tried_at(&table))
                .or_else(|| crate::making::untried_at(&made, &table))
                .cloned();
            if let Some(made) = mine {
                let game = crate::soup::of_her_own(&made, lines::time(now));
                keep(cx, t, &game);
                return AtTableNext::Done(Some(made.presentation));
            }
            t.stage = AtTableStage::Soul;
            AtTableNext::Wait(cx.effect(|id| Effect::Read {
                id,
                of: Of::Hers,
                what: ToRead::Soul,
            }))
        }
        (AtTableStage::Soul, Some(Answer::Soul { text })) => {
            t.soul = text;
            t.stage = AtTableStage::Recent;
            AtTableNext::Wait(cx.effect(|id| Effect::Read {
                id,
                of: Of::Hers,
                what: ToRead::RecentSurfaces,
            }))
        }
        (AtTableStage::Recent, Some(Answer::Surfaces { surfaces })) => {
            let settings = crate::soup::SETTINGS;
            let index =
                ((cx.at.rolls.roll() * settings.len() as f64) as usize).min(settings.len() - 1);
            t.input = crate::soup::start_input(&t.words, settings[index], &surfaces);
            make(cx, t)
        }
        (AtTableStage::Making, Some(Answer::Asked { raw, failure })) => {
            let puzzle = raw
                .as_deref()
                .and_then(crate::answer::parse::<crate::soup::Puzzle>);
            match puzzle {
                Some(puzzle) => {
                    let presentation = puzzle.presentation.trim().to_string();
                    if puzzle.surface.trim().is_empty()
                        || puzzle.truth.trim().is_empty()
                        || presentation.is_empty()
                    {
                        return AtTableNext::Done(Some(crate::soup::NOT_THIS_TIME.to_string()));
                    }
                    cx.out.effects.push(Effect::Write {
                        of: Of::Hers,
                        what: ToWrite::UsedSurface {
                            surface: puzzle.surface.chars().take(120).collect(),
                        },
                    });
                    let game = crate::soup::made_up(&puzzle, lines::time(now));
                    keep(cx, t, &game);
                    AtTableNext::Done(Some(presentation))
                }
                // No model to ask: she does not try again.
                None if failure.as_deref() == Some("unavailable") => {
                    AtTableNext::Done(Some(crate::soup::NOT_THIS_TIME.to_string()))
                }
                None if t.tries < 2 => make(cx, t),
                None => AtTableNext::Done(Some(crate::soup::NOT_THIS_TIME.to_string())),
            }
        }
        _ => AtTableNext::Done(match t.what {
            AtTableDo::Start => Some(crate::soup::NOT_THIS_TIME.to_string()),
            _ => None,
        }),
    }
}
