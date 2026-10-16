//! Whole 狼人杀 games through the site's executor: roles and night news
//! stay private, she can fill a seat, and a kept night survives a restart.

use super::seats::line_on;
use super::undercover::{first, play_out, remembered};
use super::*;
use myriad_merope::group::Seating;
use myriad_merope::hosting::SeatGame;
use myriad_merope::werewolf::{Phase, Role, Werewolf};

fn game(seating: &Seating) -> Option<&Werewolf> {
    match seating.game.as_ref()? {
        SeatGame::Werewolf(game) => Some(game),
        _ => None,
    }
}

/// Neither the group nor her voice as host gets a seat's private facts.
fn private_facts_stay_private(log: &[serde_json::Value]) {
    let end = log
        .iter()
        .position(|entry| entry["kind"] == "send" && entry.to_string().contains("赢了。身份："))
        .expect("the game ends with every role told");
    for entry in &log[..end] {
        let voice = entry["kind"] == "model" && entry["what"]["operation"] == "werewolf_voice";
        if entry["kind"] == "send" || voice {
            let text = entry.to_string();
            for secret in ["你的身份：", "狼队友", "被刀的是", "夜验"] {
                assert!(!text.contains(secret), "private fact in public: {entry}");
            }
        }
    }
    assert!(
        log.iter().any(|entry| {
            entry["kind"] == "remembered" && entry.to_string().contains("狼人杀")
        })
    );
}

/// Six humans on Telegram: private buttons, a saved first night, daytime
/// speeches and votes, then the group remembers who played and who won.
#[tokio::test(start_paused = true)]
async fn g0_werewolf_is_played_on_telegram_with_buttons() {
    let chat = -77_131;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "werewolf_is_played_on_telegram_with_buttons") else {
        return;
    };
    said(&mut world, 2.5, "好呀，我来当法官\n[[game:werewolf]]");
    answer(
        &mut world,
        "werewolf_voice",
        r#"{"say":"天黑请闭眼，大家好运"}"#,
    );
    g0::enter(world);
    hear(line(chat, 1, 81, "阿明", "@bot 来玩狼人杀", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        (81, "阿明"),
        (82, "小红"),
        (83, "阿强"),
        (84, "小李"),
        (85, "大刘"),
        (86, "老王"),
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
        &|seat, day| Some(format!("第{day}天{seat}号：我是好人")),
        &|seating, seat| {
            let game = game(seating)?;
            let view = seating.game.as_ref()?.seat_view(seat);
            if game.night == 1 && game.roles[seat] == Role::Werewolf {
                let victim = game.roles.iter().position(|role| *role == Role::Villager)?;
                return Some(seating.reachable[victim].name.clone());
            }
            let save = game.night == 1
                && view
                    .choices()
                    .get(1)
                    .is_some_and(|choice| choice.starts_with("用解药"));
            Some(if save { "1" } else { "0" }.into())
        },
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_facts_stay_private(&world.log);
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "send" && entry.to_string().contains("平安夜"))
    );
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "telegram.sendMessage"
                && entry.to_string().contains("inline_keyboard"))
    );
    g0::check("werewolf_is_played_on_telegram_with_buttons", &world.log);
    schema.drop_schema();
}

/// Five humans on QQ agree to her filling the sixth seat. A human seer
/// (or witch) never moves at night: after two misses only a private reminder
/// is sent. Her player's prompts contain only her own role.
#[tokio::test(start_paused = true)]
async fn g0_werewolf_she_fills_a_seat() {
    let chat = "77132";
    let venue = format!("onebot:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "werewolf_she_fills_a_seat") else {
        return;
    };
    said(&mut world, 2.5, "好\n[[game:werewolf]]");
    for _ in 0..40 {
        answer(
            &mut world,
            "werewolf_her_move",
            r#"{"say":"我先听听大家怎么说","n":0}"#,
        );
    }
    g0::enter(world);
    let qq = ChannelPlatform::OneBot;
    hear(line_on(qq, chat, 1, "8101", "甲", "@bot 狼人杀", true)).await;
    wait(37.5).await;
    let people = [
        ("8101", "甲"),
        ("8102", "乙"),
        ("8103", "丙"),
        ("8104", "丁"),
        ("8105", "戊"),
    ];
    for (id, (from, name)) in people.iter().enumerate() {
        hear(line_on(qq, chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    for (id, (from, name)) in people.iter().enumerate() {
        hear(line_on(qq, chat, 10 + id as i64, from, name, "好", false)).await;
    }
    play_out(
        &venue,
        qq,
        chat,
        6000.0,
        &|seat, day| Some(format!("第{day}天{seat}号：过")),
        &|seating, seat| {
            let game = game(seating)?;
            let human = |role| {
                (0..game.roles.len())
                    .find(|other| game.roles[*other] == role && !seating.reachable[*other].hers)
            };
            let idle = human(Role::Seer).or_else(|| human(Role::Witch))?;
            if seat == idle && matches!(game.phase, Phase::Night { .. }) {
                return None;
            }
            let wolf = game.roles[seat] == Role::Werewolf;
            if wolf && game.night < 3 && matches!(game.phase, Phase::Night { .. }) {
                return Some("不刀".into());
            }
            let view = seating.game.as_ref()?.seat_view(seat);
            let wanted = if wolf { Role::Villager } else { Role::Werewolf };
            let target = (0..game.roles.len()).find(|other| {
                game.alive[*other]
                    && game.roles[*other] == wanted
                    && view.choices().contains(&seating.reachable[*other].name)
            });
            Some(target.map_or_else(|| "0".into(), |other| seating.reachable[other].name.clone()))
        },
    )
    .await;
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_facts_stay_private(&world.log);
    let prompt = world
        .log
        .iter()
        .filter(|entry| {
            entry["kind"] == "model" && entry["what"]["operation"] == "werewolf_her_move"
        })
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();
    assert!(!prompt.is_empty());
    for asked in &prompt {
        let roles = ["狼人", "村民", "预言家", "女巫"]
            .into_iter()
            .filter(|role| asked.contains(&format!("你的身份：{role}")))
            .count();
        assert_eq!(
            roles, 1,
            "a player's prompt has only their own role: {asked}"
        );
    }
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "onebot.action"
                && entry["what"]["action"] == "send_private_msg"
                && entry
                    .to_string()
                    .contains(myriad_merope::hosting::REMINDER_IN_PRIVATE)),
        "private reminder"
    );
    assert!(!world.log.iter().any(|entry| {
        entry["kind"] == "send"
            && entry
                .to_string()
                .contains(myriad_merope::hosting::REMINDER_IN_PRIVATE)
    }));
    g0::check("werewolf_she_fills_a_seat", &world.log);
    schema.drop_schema();
}

/// A site restart during the first night: the stored table is restored,
/// private moves route to its seats again, and the game reaches its end.
#[tokio::test(start_paused = true)]
async fn g0_werewolf_goes_on_after_a_restart() {
    let chat = -77_133;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "werewolf_goes_on_after_a_restart") else {
        return;
    };
    world.sendable.insert(chat.to_string());
    said(&mut world, 2.5, "来\n[[game:werewolf]]");
    g0::enter(world);
    hear(line(chat, 1, 91, "阿明", "@bot 狼人杀", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        (91, "阿明"),
        (92, "小红"),
        (93, "阿强"),
        (94, "小李"),
        (95, "大刘"),
        (96, "老王"),
    ]
    .into_iter()
    .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    assert!(
        with_group(&venue, |group| group
            .seating
            .as_ref()
            .and_then(game)
            .is_some_and(|game| matches!(game.phase, Phase::Night { .. })))
        .unwrap_or(false)
    );
    g0::note("restart", json!({}));
    forget(&venue);
    take_back_tables(ChannelPlatform::Telegram).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        5000.0,
        &|seat, day| Some(format!("第{day}天{seat}号：过")),
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
    g0::check("werewolf_goes_on_after_a_restart", &world.log);
    schema.drop_schema();
}
