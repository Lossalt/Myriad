//! Whole 阿瓦隆 games through the site's executor: roles and whom each knows
//! stay private, votes on a team are told, she can fill a seat, and a kept
//! vote survives a restart.

use super::seats::line_on;
use super::undercover::{first, play_out, remembered};
use super::*;
use myriad_merope::avalon::{Avalon, Phase};
use myriad_merope::group::Seating;
use myriad_merope::hosting::SeatGame;

fn game(seating: &Seating) -> Option<&Avalon> {
    match seating.game.as_ref()? {
        SeatGame::Avalon(game) => Some(game),
        _ => None,
    }
}

/// How the scene's players play: the leader takes the first one open each
/// pick; the first team of the game is turned down, every other one goes;
/// the evil fail the first quest only; the assassin names the first open.
fn player(seating: &Seating, seat: usize) -> Option<String> {
    let game = game(seating)?;
    Some(match &game.phase {
        Phase::Vote { .. } if game.results.is_empty() && game.rejections == 0 => "1".into(),
        Phase::Quest { .. } if game.results.is_empty() && game.roles[seat].evil() => "1".into(),
        _ => "0".into(),
    })
}

/// Neither the group nor her voice as host gets what a seat was dealt.
fn private_facts_stay_private(log: &[serde_json::Value]) {
    let end = log
        .iter()
        .position(|entry| entry["kind"] == "send" && entry.to_string().contains("赢了。身份："))
        .expect("the game ends with every role told");
    for entry in &log[..end] {
        let voice = entry["kind"] == "model" && entry["what"]["operation"] == "avalon_voice";
        if entry["kind"] == "send" || voice {
            let text = entry.to_string();
            for secret in ["你的身份：", "你看到的坏人", "坏人同伴", "一个是梅林"]
            {
                assert!(!text.contains(secret), "private fact in public: {entry}");
            }
        }
    }
    assert!(
        log.iter()
            .any(|entry| entry["kind"] == "remembered" && entry.to_string().contains("阿瓦隆"))
    );
}

/// Five humans on Telegram: private buttons, the first team turned down
/// and how everyone voted told, a quest failed by one card, the good
/// winning three and the assassin naming someone; the group remembers it.
#[tokio::test(start_paused = true)]
async fn g0_avalon_is_played_on_telegram_with_buttons() {
    let chat = -77_141;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "avalon_is_played_on_telegram_with_buttons") else {
        return;
    };
    said(&mut world, 2.5, "好呀，我来主持\n[[game:avalon]]");
    answer(
        &mut world,
        "avalon_voice",
        r#"{"say":"为了亚瑟王，出发吧"}"#,
    );
    g0::enter(world);
    hear(line(chat, 1, 101, "阿明", "@bot 来玩阿瓦隆", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        (101, "阿明"),
        (102, "小红"),
        (103, "阿强"),
        (104, "小李"),
        (105, "大刘"),
    ]
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
        5000.0,
        &|_, _| None,
        &player,
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_facts_stay_private(&world.log);
    let told: Vec<String> = world
        .log
        .iter()
        .filter(|entry| entry["kind"] == "send")
        .map(|entry| entry.to_string())
        .collect();
    assert!(
        told.iter()
            .any(|line| line.contains("没过半，这次没组成队"))
    );
    assert!(told.iter().any(|line| line.contains("任务失败")));
    assert!(told.iter().any(|line| line.contains("刺杀了")));
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "telegram.sendMessage"
                && entry.to_string().contains("inline_keyboard"))
    );
    g0::check("avalon_is_played_on_telegram_with_buttons", &world.log);
    schema.drop_schema();
}

/// Four humans on QQ agree to her filling the fifth seat. She picks, votes
/// and plays from her own seat's view: what she is asked as her seat holds
/// her own role and no one else's.
#[tokio::test(start_paused = true)]
async fn g0_avalon_she_fills_a_seat() {
    let chat = "77142";
    let venue = format!("onebot:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "avalon_she_fills_a_seat") else {
        return;
    };
    said(&mut world, 2.5, "好\n[[game:avalon]]");
    for _ in 0..60 {
        answer(&mut world, "avalon_her_move", r#"{"n":0}"#);
    }
    g0::enter(world);
    let qq = ChannelPlatform::OneBot;
    hear(line_on(qq, chat, 1, "9101", "甲", "@bot 阿瓦隆", true)).await;
    wait(37.5).await;
    let people = [
        ("9101", "甲"),
        ("9102", "乙"),
        ("9103", "丙"),
        ("9104", "丁"),
    ];
    for (id, (from, name)) in people.iter().enumerate() {
        hear(line_on(qq, chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    for (id, (from, name)) in people.iter().enumerate() {
        hear(line_on(qq, chat, 10 + id as i64, from, name, "好", false)).await;
    }
    play_out(&venue, qq, chat, 6000.0, &|_, _| None, &player).await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_facts_stay_private(&world.log);
    let prompts: Vec<String> = world
        .log
        .iter()
        .filter(|entry| entry["kind"] == "model" && entry["what"]["operation"] == "avalon_her_move")
        .map(|entry| entry.to_string())
        .collect();
    assert!(!prompts.is_empty());
    for asked in &prompts {
        let roles = ["梅林", "派西维尔", "亚瑟的忠臣", "莫甘娜", "刺客"]
            .into_iter()
            .filter(|role| asked.contains(&format!("你的身份：{role}")))
            .count();
        assert_eq!(
            roles, 1,
            "a player's prompt has only their own role: {asked}"
        );
    }
    g0::check("avalon_she_fills_a_seat", &world.log);
    schema.drop_schema();
}

/// A site restart while the first team is voted on: the stored table is
/// restored, the votes already whispered stand, the others' still reach
/// the game, and it is played to its end.
#[tokio::test(start_paused = true)]
async fn g0_avalon_goes_on_after_a_restart() {
    let chat = -77_143;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "avalon_goes_on_after_a_restart") else {
        return;
    };
    world.sendable.insert(chat.to_string());
    said(&mut world, 2.5, "来\n[[game:avalon]]");
    g0::enter(world);
    hear(line(chat, 1, 111, "阿明", "@bot 阿瓦隆", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        (111, "阿明"),
        (112, "小红"),
        (113, "阿强"),
        (114, "小李"),
        (115, "大刘"),
    ]
    .into_iter()
    .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    // Until the first team is being voted on, and some voted.
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        40.0,
        &|_, _| None,
        &|seating, seat| match game(seating)?.phase {
            Phase::Vote { .. } if seat > 1 => None,
            _ => Some("0".into()),
        },
    )
    .await;
    assert!(
        with_group(&venue, |group| group
            .seating
            .as_ref()
            .and_then(game)
            .is_some_and(
                |game| matches!(&game.phase, Phase::Vote { votes } if !votes.is_empty())
            ))
        .unwrap_or(false),
        "the restart comes while the first team is voted on"
    );
    g0::note("restart", json!({}));
    forget(&venue);
    take_back_tables(ChannelPlatform::Telegram).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        5000.0,
        &|_, _| None,
        &first,
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_facts_stay_private(&world.log);
    let restart = world
        .log
        .iter()
        .position(|entry| entry["kind"] == "restart")
        .unwrap();
    assert!(
        world.log[restart..]
            .iter()
            .any(|entry| entry["kind"] == "send" && entry.to_string().contains("赢了。身份："))
    );
    g0::check("avalon_goes_on_after_a_restart", &world.log);
    schema.drop_schema();
}
