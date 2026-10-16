//! G0 scenes of 谁是卧底 at a group's table (D2c): a whole game on
//! Telegram (votes with buttons) and on QQ (votes typed as numbers or
//! names), her taking the fourth seat when all agree, a tie, a game that
//! goes on after a restart, and descriptions that give their word away.
//! The players are played as people would: whoever's turn it is says a
//! line in the group, whoever has to vote whispers it.

use super::seats::{line_on, nothing_dealt_said_in_the_group, whisper_in};
use super::*;
use myriad_merope::group::Seating;
use myriad_merope::hosting::Waiting;

/// The table at `venue` as it stands, if a game is on there.
fn table(venue: &str) -> Option<Seating> {
    with_group(venue, |group| group.seating.clone()).flatten()
}

/// The players at `venue`'s table, every 9.7 seconds until `until` (from
/// now): whoever's turn it is to speak says `said(seat, round)` in the
/// group (none: lets it pass); whoever has to choose whispers `chosen`.
pub(super) async fn play_out(
    venue: &str,
    platform: ChannelPlatform,
    chat: &str,
    until: f64,
    said: &dyn Fn(usize, u32) -> Option<String>,
    chosen: &dyn Fn(&Seating, usize) -> Option<String>,
) {
    let mut id = 500;
    let mut at = 0.0;
    while at < until {
        wait(9.7).await;
        at += 9.7;
        let Some(seating) = table(venue) else {
            break;
        };
        let Some(game) = seating.game.as_ref() else {
            continue;
        };
        let round = game.round();
        match game.waiting() {
            Waiting::Speak(seat) if !seating.reachable[seat].hers => {
                let sat = &seating.reachable[seat];
                let user = sat.who.split_once(':').map_or("", |(_, user)| user);
                if let Some(text) = said(seat, round) {
                    id += 1;
                    hear(line_on(platform, chat, id, user, &sat.name, &text, false)).await;
                }
            }
            Waiting::Choose(seats) => {
                for seat in seats {
                    let sat = &seating.reachable[seat];
                    if sat.hers {
                        continue;
                    }
                    let user = sat.who.split_once(':').map_or("", |(_, user)| user);
                    if let Some(text) = chosen(&seating, seat) {
                        whisper_in(platform, user, &text).await;
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn first(_: &Seating, _: usize) -> Option<String> {
    Some("0".into())
}

/// What the scene's table was dealt, and what the group remembers of it.
pub(super) async fn remembered(db: &sea_orm::DatabaseConnection, venue: &str) {
    use sea_orm::ConnectionTrait;
    let rows = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT content FROM agent_memories WHERE venue = $1 AND source = 'game' ORDER BY created_at",
            [format!("group:{venue}").into()],
        ))
        .await
        .unwrap();
    let kept: Vec<String> = rows
        .iter()
        .map(|row| row.try_get::<String>("", "content").unwrap_or_default())
        .collect();
    g0::note("remembered", json!(kept));
}

fn judged(world: &mut World, times: usize) {
    for _ in 0..times {
        answer(world, "undercover_judge", r#"{"givesAway":false}"#);
    }
}

/// Someone from outside asks for 谁是卧底 in a Telegram group: four join
/// after her sign-up line, each is whispered their own word (made fresh,
/// not a pair used lately), she tells the rules and says a line of her own,
/// each describes in the group in turn (one asks for more time once),
/// votes come back from buttons, until a side wins and both words are told;
/// the group remembers who played and how it went. Nothing dealt is said
/// in the group before the end.
#[tokio::test(start_paused = true)]
async fn g0_undercover_is_played_on_telegram_with_buttons() {
    let chat = -77_121;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "undercover_is_played_on_telegram_with_buttons") else {
        return;
    };
    world.registry.insert(
        ("merope_game_dealt".into(), "undercover".into()),
        json!(["饺子/包子"]),
    );
    said(&mut world, 2.5, "好呀，我来当法官\n[[game:undercover]]");
    answer(
        &mut world,
        "undercover_words",
        r#"{"civilian":"眼镜","undercover":"墨镜"}"#,
    );
    for line in ["人到齐了，开始吧", "猜错一个", "这么快就结束了"] {
        answer(
            &mut world,
            "undercover_voice",
            &json!({ "say": line }).to_string(),
        );
    }
    judged(&mut world, 20);
    g0::enter(world);
    hear(line(chat, 1, 41, "路人", "@bot 来玩谁是卧底", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [(41, "路人"), (42, "小李"), (43, "阿强"), (44, "小红")]
        .into_iter()
        .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    // Sign-up closes, everyone is whispered to, the words are dealt.
    wait(45.3).await;
    hear(line(chat, 9, 43, "阿强", "等等", false)).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        1500.0,
        &|seat, round| Some(format!("戴在脸上的，第{round}轮{seat}")),
        &first,
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("undercover_is_played_on_telegram_with_buttons", &world.log);
    nothing_dealt_said_in_the_group(&world.log);
    words_kept_back_until_the_end(&world.log, &["眼镜", "墨镜"]);
    schema.drop_schema();
}

/// Neither word is said in the group, nor asked of her as the game's
/// voice, before the line that ends it.
fn words_kept_back_until_the_end(log: &[serde_json::Value], words: &[&str]) {
    let end = log
        .iter()
        .position(|entry| entry["kind"] == "send" && entry.to_string().contains("赢了"))
        .expect("it ends");
    for entry in &log[..end] {
        let kind = entry["kind"].as_str().unwrap_or("");
        let voice = kind == "model" && entry["what"]["operation"] == "undercover_voice";
        if kind == "send" || voice {
            for word in words {
                assert!(
                    !entry.to_string().contains(word),
                    "{word} before the end: {entry}"
                );
            }
        }
    }
}

/// The same on QQ: no buttons, votes typed back as a number or a name; one
/// who is not her friend is told to add her; one types a number when it is
/// not their turn and is told so.
#[tokio::test(start_paused = true)]
async fn g0_undercover_is_played_on_qq_with_numbers() {
    let chat = "77122";
    let venue = format!("onebot:{chat}");
    let mut world = World::new(0.3);
    let Some(schema) = database(&mut world, "undercover_is_played_on_qq_with_numbers") else {
        return;
    };
    world.unreachable.insert("onebot:6105".into());
    said(&mut world, 2.5, "来\n[[game:undercover]]");
    answer(
        &mut world,
        "undercover_words",
        r#"{"civilian":"牙刷","undercover":"牙膏"}"#,
    );
    judged(&mut world, 20);
    g0::enter(world);
    let qq = ChannelPlatform::OneBot;
    hear(line_on(qq, chat, 1, "6101", "甲", "@bot 玩谁是卧底", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        ("6101", "甲"),
        ("6102", "乙"),
        ("6103", "丙"),
        ("6104", "丁"),
        ("6105", "戊"),
    ]
    .into_iter()
    .enumerate()
    {
        hear(line_on(qq, chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    whisper_in(qq, "6102", "1").await;
    play_out(
        &venue,
        qq,
        chat,
        1500.0,
        &|seat, round| Some(format!("每天早上都用，第{round}轮{seat}")),
        // Some type the name of whom they vote for, some its number.
        &|seating, seat| {
            let view = seating.game.as_ref()?.seat_view(seat);
            Some(if seat % 2 == 0 {
                view.choices().first()?.clone()
            } else {
                "0".into()
            })
        },
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("undercover_is_played_on_qq_with_numbers", &world.log);
    nothing_dealt_said_in_the_group(&world.log);
    words_kept_back_until_the_end(&world.log, &["牙刷", "牙膏"]);
    schema.drop_schema();
}

/// Three join: she offers to take the fourth seat, and all three agree.
/// The words are made by a judge with nothing of her; she describes in her
/// turn and votes, from her own word alone. One who lets two turns in a
/// row pass is named in the group.
#[tokio::test(start_paused = true)]
async fn g0_undercover_she_fills_a_seat() {
    let chat = -77_123;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.6);
    let Some(schema) = database(&mut world, "undercover_she_fills_a_seat") else {
        return;
    };
    said(&mut world, 2.5, "好\n[[game:undercover]]");
    answer(
        &mut world,
        "undercover_words",
        r#"{"civilian":"雨伞","undercover":"雨衣"}"#,
    );
    for _ in 0..8 {
        answer(
            &mut world,
            "undercover_her_move",
            r#"{"say":"下雨天会想到它","n":0}"#,
        );
    }
    judged(&mut world, 20);
    g0::enter(world);
    hear(line(chat, 1, 51, "阿明", "@bot 谁是卧底来不来", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [(51, "阿明"), (52, "小红"), (53, "阿强")]
        .into_iter()
        .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    for (id, (from, name)) in [(51, "阿明"), (52, "小红"), (53, "阿强")]
        .into_iter()
        .enumerate()
    {
        hear(line(chat, 10 + id as i64, from, name, "好", false)).await;
    }
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        1500.0,
        // 阿强 neither describes nor votes.
        &|seat, round| (seat != 2).then(|| format!("出门可能会带，第{round}轮")),
        &|_, seat| (seat != 2).then(|| "0".into()),
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("undercover_she_fills_a_seat", &world.log);
    nothing_dealt_said_in_the_group(&world.log);
    words_kept_back_until_the_end(&world.log, &["雨伞", "雨衣"]);
    // What she is asked as her seat holds her own word, never the other;
    // the words were made with nothing of her.
    let asked: Vec<&serde_json::Value> = world
        .log
        .iter()
        .filter(|entry| entry["kind"] == "model")
        .collect();
    let mine: Vec<String> = asked
        .iter()
        .filter(|entry| entry["what"]["operation"] == "undercover_her_move")
        .map(|entry| entry.to_string())
        .collect();
    assert!(!mine.is_empty());
    let hers = mine[0].contains("雨伞");
    let other = if hers { "雨衣" } else { "雨伞" };
    assert!(mine.iter().all(|asked| !asked.contains(other)));
    assert!(asked.iter().any(|entry| {
        entry["what"]["operation"] == "undercover_words"
            && !entry["what"]["system"]
                .as_str()
                .unwrap_or("")
                .contains(g0::SOUL)
    }));
    schema.drop_schema();
}

/// A tie: the two tied describe once more and the others vote between
/// them; tied again, nobody goes out that round and the next begins.
#[tokio::test(start_paused = true)]
async fn g0_undercover_a_tie() {
    let chat = -77_124;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.4);
    let Some(schema) = database(&mut world, "undercover_a_tie") else {
        return;
    };
    said(&mut world, 2.5, "来一局\n[[game:undercover]]");
    answer(
        &mut world,
        "undercover_words",
        r#"{"civilian":"火锅","undercover":"麻辣烫"}"#,
    );
    judged(&mut world, 30);
    g0::enter(world);
    hear(line(chat, 1, 61, "一号", "@bot 谁是卧底", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [(61, "一号"), (62, "二号"), (63, "三号"), (64, "四号")]
        .into_iter()
        .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    // In the first round 一号 and 二号 get two votes each, and again when
    // the others vote between them; after that, everyone votes the first.
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        1500.0,
        &|seat, round| Some(format!("冬天想吃，第{round}轮{seat}")),
        &|seating, seat| {
            let myriad_merope::hosting::SeatGame::Undercover(game) = seating.game.as_ref()? else {
                return None;
            };
            if game.round > 1 {
                return Some("0".into());
            }
            let name = |index: usize| seating.reachable[index].name.clone();
            Some(match seat {
                0 => name(1),
                1 => name(0),
                2 => name(0),
                _ => name(1),
            })
        },
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("undercover_a_tie", &world.log);
    let told: Vec<String> = world
        .log
        .iter()
        .filter(|entry| entry["kind"] == "send")
        .map(|entry| entry.to_string())
        .collect();
    assert!(told.iter().any(|line| line.contains("平票")));
    assert!(
        told.iter()
            .any(|line| line.contains("又平票，这轮没人出局"))
    );
    schema.drop_schema();
}

/// A game is on when the site restarts: the table comes back as kept, who
/// sits at it is seated again (their whispered votes reach the game, not
/// pairing), the wait goes on for the time it had left, and it is played
/// to its end.
#[tokio::test(start_paused = true)]
async fn g0_undercover_goes_on_after_a_restart() {
    let chat = -77_125;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "undercover_goes_on_after_a_restart") else {
        return;
    };
    world.sendable.insert(chat.to_string());
    said(&mut world, 2.5, "好\n[[game:undercover]]");
    answer(
        &mut world,
        "undercover_words",
        r#"{"civilian":"手机","undercover":"平板"}"#,
    );
    judged(&mut world, 20);
    g0::enter(world);
    hear(line(chat, 1, 71, "阿明", "@bot 谁是卧底", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [(71, "阿明"), (72, "小红"), (73, "阿强"), (74, "小李")]
        .into_iter()
        .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        30.0,
        &|seat, round| Some(format!("天天拿在手里，第{round}轮{seat}")),
        &first,
    )
    .await;
    // The restart: her mind of the group and who sits where are gone; what
    // was kept is not.
    g0::note("restart", json!({}));
    forget(&venue);
    take_back_tables(ChannelPlatform::Telegram).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        1500.0,
        &|seat, round| Some(format!("天天拿在手里，第{round}轮{seat}")),
        &first,
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("undercover_goes_on_after_a_restart", &world.log);
    let restart = world
        .log
        .iter()
        .position(|entry| entry["kind"] == "restart")
        .unwrap();
    assert!(
        world.log[restart..]
            .iter()
            .any(|entry| entry["kind"] == "send" && entry.to_string().contains("赢了")),
        "played to its end after the restart"
    );
    schema.drop_schema();
}

/// Descriptions that give their word away: one says it outright and is out
/// at once; another writes a homophone of it, which the judge sees, and is
/// out too. (In a recording every roll is the same, so the undercover is
/// the first to describe.)
#[tokio::test(start_paused = true)]
async fn g0_undercover_a_description_gives_the_word_away() {
    let chat = -77_126;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "undercover_a_description_gives_the_word_away") else {
        return;
    };
    said(&mut world, 2.5, "来\n[[game:undercover]]");
    answer(
        &mut world,
        "undercover_words",
        r#"{"civilian":"西瓜","undercover":"哈密瓜"}"#,
    );
    // The first line is fine; the second says the word outright (no judge
    // needed); the third is a homophone, which the judge sees.
    judged(&mut world, 1);
    answer(&mut world, "undercover_judge", r#"{"givesAway":true}"#);
    judged(&mut world, 20);
    g0::enter(world);
    hear(line(chat, 1, 81, "甲", "@bot 谁是卧底", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [(81, "甲"), (82, "乙"), (83, "丙"), (84, "丁"), (85, "戊")]
        .into_iter()
        .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    let words = std::sync::Mutex::new(0);
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        1500.0,
        &|_, round| {
            let mut said = words.lock().unwrap();
            *said += 1;
            Some(match *said {
                // The second says the word outright; the third writes a
                // homophone of it.
                2 => "就是西瓜嘛".into(),
                3 => "希瓜".into(),
                _ => format!("夏天吃的，第{round}轮"),
            })
        },
        &first,
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("undercover_a_description_gives_the_word_away", &world.log);
    let told: Vec<String> = world
        .log
        .iter()
        .filter(|entry| entry["kind"] == "send")
        .map(|entry| entry.to_string())
        .collect();
    assert!(
        told.iter()
            .any(|line| line.contains("说出了自己的词，出局"))
    );
    assert!(
        told.iter()
            .any(|line| line.contains("这句等于说出了自己的词，出局"))
    );
    schema.drop_schema();
}
