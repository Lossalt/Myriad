//! G0 scenes of her answering in a group: members calling her, waiting
//! their turn, being kept back, muted, away, read back, pictures, firsts.

use super::*;

/// A member calls her: she sees it after a while, reads the group, answers
/// in her own turn, and her words go out typed.
#[tokio::test(start_paused = true)]
async fn g0_a_member_calls_her() {
    let chat = -77_001;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "a_member_calls_her") else {
        return;
    };
    world.members.insert("11".into(), 70011);
    site_member(&schema.db, 70011, "u70011", "阿明").await;
    // Not a multiple of the typing mark's 4 s: a tie there has no order.
    her_turn(&mut world, 4.5, "去老地方吧，便宜又好吃");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "周五聚餐吗", false)).await;
    wait(2.0).await;
    hear(line(chat, 2, 12, "小红", "我可以", false)).await;
    wait(2.0).await;
    hear(line(chat, 3, 11, "阿明", "@bot 你推荐哪家", true)).await;
    wait(600.0).await;
    done("a_member_calls_her");
    schema.drop_schema();
}

/// Called while she is answering: the line waits, and the same person's
/// added line is answered with theirs.
#[tokio::test(start_paused = true)]
async fn g0_lines_wait_while_she_is_busy() {
    let chat = -77_002;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.3);
    let Some(schema) = database(&mut world, "lines_wait_while_she_is_busy") else {
        return;
    };
    world.members.insert("11".into(), 70021);
    world.members.insert("12".into(), 70022);
    site_member(&schema.db, 70021, "u70021", "阿明").await;
    site_member(&schema.db, 70022, "u70022", "小红").await;
    her_turn(&mut world, 20.5, "我先看看");
    her_turn(&mut world, 3.0, "火锅可以");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 周五去哪", true)).await;
    wait(15.0).await;
    hear(line(chat, 2, 12, "小红", "@bot 你吃火锅吗", true)).await;
    wait(1.0).await;
    hear(line(chat, 3, 12, "小红", "要辣的", false)).await;
    wait(900.0).await;
    done("lines_wait_while_she_is_busy");
    schema.drop_schema();
}

/// Nobody calls her: she glances when she is free, judges as herself, and
/// speaks up; five minutes on it is kept whether anyone took it up.
#[tokio::test(start_paused = true)]
async fn g0_she_speaks_up_unasked() {
    let chat = -77_003;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "she_speaks_up_unasked") else {
        return;
    };
    world.members.insert("11".into(), 70031);
    site_member(&schema.db, 70031, "u70031", "阿明").await;
    answer(
        &mut world,
        "group_chime",
        r#"{"speak":"know","about":"那家店周五人多","basis":null}"#,
    );
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"在约周五吃饭","theyMean":"在问去哪家","aboutYou":null}"#,
    );
    her_turn(&mut world, 3.0, "周五那家人多，早点去");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "周五吃那家火锅吧", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 11, "阿明", "大家觉得呢", false)).await;
    wait(1200.0).await;
    done("she_speaks_up_unasked");
    schema.drop_schema();
}

/// No words, only a reaction on the line.
#[tokio::test(start_paused = true)]
async fn g0_she_reacts_without_words() {
    let chat = -77_004;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "she_reacts_without_words") else {
        return;
    };
    world.members.insert("11".into(), 70041);
    site_member(&schema.db, 70041, "u70041", "阿明").await;
    answer(
        &mut world,
        "group_chime",
        r#"{"speak":"react","about":"😁"}"#,
    );
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "我今天把锅烧糊了哈哈哈", false)).await;
    wait(600.0).await;
    done("she_reacts_without_words");
    schema.drop_schema();
}

/// Someone from outside the community calls her: answered lightly, on the
/// owner's budget, and the exchange is counted after.
#[tokio::test(start_paused = true)]
async fn g0_an_outsider_calls_her() {
    let chat = -77_005;
    let mut world = World::new(0.7);
    let Some(schema) = database(&mut world, "an_outsider_calls_her") else {
        return;
    };
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"有人第一次来","theyMean":"问她是谁","aboutYou":"说她像机器人"}"#,
    );
    world
        .says
        .entry("group_stranger".into())
        .or_default()
        .push_back((2.0, Ok("我是这里的常客啦".into())));
    g0::enter(world);
    hear(line(chat, 1, 31, "路人", "这个bot是谁", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 31, "路人", "@bot 你是机器人吗", true)).await;
    wait(600.0).await;
    done("an_outsider_calls_her");
    schema.drop_schema();
}

/// The talk moved on while she wrote: she looks again, keeps it back, and
/// answers once more from the newest line.
#[tokio::test(start_paused = true)]
async fn g0_she_keeps_back_what_no_longer_fits() {
    let chat = -77_006;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "she_keeps_back_what_no_longer_fits") else {
        return;
    };
    world.members.insert("11".into(), 70061);
    world.members.insert("12".into(), 70062);
    site_member(&schema.db, 70061, "u70061", "阿明").await;
    site_member(&schema.db, 70062, "u70062", "小红").await;
    her_turn(&mut world, 10.0, "九点吧\n老地方见");
    her_turn(&mut world, 2.0, "那就十点");
    answer(&mut world, "draft_still", r#"{"send":false}"#);
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 周六几点集合", true)).await;
    wait(9.0).await;
    hear(line(chat, 2, 12, "小红", "刚说了十点", false)).await;
    wait(600.0).await;
    done("she_keeps_back_what_no_longer_fits");
    schema.drop_schema();
}

/// Several lines in one reply, the rest typed one after another.
#[tokio::test(start_paused = true)]
async fn g0_a_reply_goes_out_as_several_messages() {
    let chat = -77_007;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.8);
    let Some(schema) = database(&mut world, "a_reply_goes_out_as_several_messages") else {
        return;
    };
    world.members.insert("11".into(), 70071);
    site_member(&schema.db, 70071, "u70071", "阿明").await;
    her_turn(&mut world, 2.0, "好耶\n我也想去\n带我一个\n@阿明 几点");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 周六去爬山", true)).await;
    wait(600.0).await;
    done("a_reply_goes_out_as_several_messages");
    schema.drop_schema();
}

/// Muted, she knows it and says nothing there until it lifts.
#[tokio::test(start_paused = true)]
async fn g0_muted_she_says_nothing() {
    let chat = -77_008;
    let venue = format!("telegram:{chat}");
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "muted_she_says_nothing") else {
        return;
    };
    world.members.insert("11".into(), 70081);
    site_member(&schema.db, 70081, "u70081", "阿明").await;
    her_turn(&mut world, 2.0, "我又能说话啦");
    g0::enter(world);
    hear(line(chat, 1, 12, "管理员", "安静点", false)).await;
    muted(&venue, "12", Muted::For(600), false).await;
    hear(line(chat, 2, 11, "阿明", "@bot 你还在吗", true)).await;
    wait(700.0).await;
    hear(line(chat, 3, 11, "阿明", "@bot 现在呢", true)).await;
    wait(600.0).await;
    done("muted_she_says_nothing");
    schema.drop_schema();
}

/// One who answers her nonstop, as a bot would: after enough rounds she
/// stops answering them for a while.
#[tokio::test(start_paused = true)]
async fn g0_she_stops_answering_one_who_never_stops() {
    let chat = -77_009;
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "she_stops_answering_one_who_never_stops") else {
        return;
    };
    for _ in 0..myriad_merope::group::LOOP_ROUNDS + 2 {
        world
            .says
            .entry("group_stranger".into())
            .or_default()
            .push_back((1.0, Ok("在".into())));
    }
    g0::enter(world);
    for id in 1..=(myriad_merope::group::LOOP_ROUNDS as i64 + 2) {
        hear(line(chat, id, 77, "复读机", "@bot 在吗", true)).await;
        wait(25.0).await;
    }
    wait(600.0).await;
    done("she_stops_answering_one_who_never_stops");
    schema.drop_schema();
}

/// Away from the site she is not in the group; back, she takes up the
/// latest line that called her.
#[tokio::test(start_paused = true)]
async fn g0_away_and_back() {
    let chat = -77_010;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "away_and_back") else {
        return;
    };
    world.away = true;
    world.members.insert("11".into(), 70101);
    site_member(&schema.db, 70101, "u70101", "阿明").await;
    her_turn(&mut world, 2.0, "回来了回来了");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 在吗", true)).await;
    wait(5.0).await;
    hear(line(chat, 2, 11, "阿明", "人呢", false)).await;
    wait(5.0).await;
    hear(line(chat, 3, 11, "阿明", "@bot 回来说一声", true)).await;
    wait(300.0).await;
    g0::with(|world| world.away = false);
    back_from_away();
    wait(600.0).await;
    done("away_and_back");
    schema.drop_schema();
}

/// After a restart the group is read back as a person scrolls up, and the
/// line that called her and that she never got to is taken up, late.
#[tokio::test(start_paused = true)]
async fn g0_read_back_after_a_restart() {
    let chat = -77_011;
    let venue = format!("telegram:{chat}");
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "read_back_after_a_restart") else {
        return;
    };
    world.members.insert("11".into(), 70111);
    site_member(&schema.db, 70111, "u70111", "阿明").await;
    her_turn(&mut world, 2.0, "抱歉刚看到");
    let base = world.base;
    world.up_since = Some(base - chrono::Duration::minutes(5));
    g0::enter(world);
    let ago = |minutes: i64| base - chrono::Duration::minutes(minutes);
    catch_up(
        &venue,
        vec![
            (line(chat, 1, 11, "阿明", "@bot 早", true), ago(90), false),
            (line(chat, 2, 0, "", "早呀", false), ago(89), true),
            (line(chat, 3, 12, "小红", "她起了没", false), ago(40), false),
            (
                line(chat, 4, 11, "阿明", "@bot 晚上打游戏吗", true),
                ago(30),
                false,
            ),
            (
                line(chat, 5, 12, "小红", "没回，睡着了吧", false),
                ago(20),
                false,
            ),
        ],
    )
    .await;
    wait(600.0).await;
    done("read_back_after_a_restart");
    schema.drop_schema();
}

/// Pictures in the talk: looked at when she reads it, said as she saw
/// them, and one sent again is the group's.
#[tokio::test(start_paused = true)]
async fn g0_pictures_as_she_saw_them() {
    let chat = -77_012;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "pictures_as_she_saw_them") else {
        return;
    };
    world.members.insert("11".into(), 70121);
    site_member(&schema.db, 70121, "u70121", "阿明").await;
    world.seen.insert(
        "telegram:cat".into(),
        myriad_merope::seeing::Seen {
            what: "一只翻白眼的猫".into(),
            says: Some("无语".into()),
            keep: true,
            only_there: false,
        },
    );
    her_turn(&mut world, 2.0, "这猫好懂你");
    g0::enter(world);
    let picture = |id: i64| {
        let mut line = line(chat, id, 11, "阿明", "", false);
        line.images = vec![GroupImage {
            key: "telegram:cat".into(),
            fetch: ImageFetch::TelegramFile {
                file_id: "file-cat".into(),
            },
            hint: Some("😂".into()),
            sticker: true,
        }];
        line
    };
    hear(picture(1)).await;
    wait(1.0).await;
    hear(line(chat, 2, 12, "小红", "哈哈哈", false)).await;
    wait(1.0).await;
    hear(picture(3)).await;
    wait(1.0).await;
    hear(line(chat, 4, 11, "阿明", "@bot 你看这个", true)).await;
    wait(600.0).await;
    done("pictures_as_she_saw_them");
    schema.drop_schema();
}

/// Enough of the group's talk goes by: what she heard in it is taken in.
#[tokio::test(start_paused = true)]
async fn g0_what_she_heard_is_taken_in() {
    let chat = -77_013;
    let world = World::new(0.5);
    g0::enter(world);
    for id in 0..myriad_merope::group::HEARD_EVERY as i64 {
        let from = 11 + id % 3;
        hear(line(
            chat,
            id + 1,
            from,
            &format!("人{from}"),
            &format!("第{id}句：新开的店不错"),
            false,
        ))
        .await;
        wait(3.0).await;
    }
    wait(600.0).await;
    done("what_she_heard_is_taken_in");
}

/// Something of hers she wants to tell: she looks over the groups she is
/// in and brings it up in one, unasked.
#[tokio::test(start_paused = true)]
async fn g0_she_brings_something_up_first() {
    let chat = -77_014;
    let venue = format!("telegram:{chat}");
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "she_brings_something_up_first") else {
        return;
    };
    world.members.insert("11".into(), 70141);
    site_member(&schema.db, 70141, "u70141", "阿明").await;
    site_member(&schema.db, 1, "owner", "站长").await;
    world.sendable.insert(chat.to_string());
    her_turn(&mut world, 2.0, "今天听到一首好歌，分享给你们");
    answer(
        &mut world,
        "share_first",
        &json!({ "group": venue, "why": "大家前几天在聊歌" }).to_string(),
    );
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "最近有什么好歌", false)).await;
    wait(1800.0).await;
    share_first(1, "一首叫《夜航》的歌".into()).await;
    wait(600.0).await;
    done("she_brings_something_up_first");
    schema.drop_schema();
}

/// Only a sticker: it goes, and she and the group see she sent it.
#[tokio::test(start_paused = true)]
async fn g0_only_a_sticker() {
    let chat = -77_015;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "only_a_sticker") else {
        return;
    };
    world.members.insert("11".into(), 70151);
    site_member(&schema.db, 70151, "u70151", "阿明").await;
    // Two stickers she kept from what people sent; she sends the second.
    {
        use sea_orm::ConnectionTrait;
        schema
            .db
            .execute_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO merope_stickers (id, picture, meaning, venue, asset_id, identity, sent, kept_from) VALUES \
                 ('sticker-15a', '一只发呆的猫', '无聊', NULL, 'asset-15a', '', 3, 'telegram:-77199'), \
                 ('sticker-15b', '一只跳起来的猫', '开心', NULL, 'asset-15b', '', 1, 'telegram:-77199')",
            ))
            .await
            .unwrap();
    }
    her_turn(&mut world, 2.0, "[[sticker:2]]");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 发个表情", true)).await;
    wait(600.0).await;
    done("only_a_sticker");
    schema.drop_schema();
}

/// The platform refuses a send: what went out is what she said there.
#[tokio::test(start_paused = true)]
async fn g0_a_send_is_refused() {
    let chat = -77_016;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "a_send_is_refused") else {
        return;
    };
    world.members.insert("11".into(), 70161);
    site_member(&schema.db, 70161, "u70161", "阿明").await;
    her_turn(&mut world, 2.0, "第一句\n第二句");
    world.refused.insert(1);
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 说两句", true)).await;
    wait(600.0).await;
    done("a_send_is_refused");
    schema.drop_schema();
}

/// The database is down when the first line after a restart comes: what
/// was kept before is neither read back nor overwritten. Up again by the
/// time she answers, her own line brings it back, under what was said since.
#[tokio::test(start_paused = true)]
async fn g0_her_line_brings_back_what_a_failed_read_missed() {
    let chat = -77_017;
    let venue = format!("telegram:{chat}");
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "her_line_brings_back_what_a_failed_read_missed")
    else {
        return;
    };
    world.members.insert("11".into(), 70171);
    site_member(&schema.db, 70171, "u70171", "阿明").await;
    her_turn(&mut world, 2.0, "记得，你说要来点歌");
    her_turn(&mut world, 2.0, "那就放这首");
    let base = world.base;
    let before = |minutes: i64| (base - chrono::Duration::minutes(minutes)).to_rfc3339();
    world.registry.insert(
        ("group_lines".into(), venue.clone()),
        json!({ "lines": [
            { "at": before(40), "message_id": "1", "name": "阿明", "from": "11",
              "text": "来点歌", "hers": false, "addressed": false },
            { "at": before(39), "message_id": null, "name": "", "text": "放就放", "hers": true },
        ]}),
    );
    world
        .failing
        .insert(("read".into(), "group_lines".into()), 1);
    world
        .failing
        .insert(("write".into(), "group_lines".into()), 1);
    g0::enter(world);
    hear(line(chat, 3, 11, "阿明", "@bot 还记得我刚才说的吗", true)).await;
    wait(600.0).await;
    hear(line(chat, 4, 11, "阿明", "@bot 那放哪首", true)).await;
    wait(600.0).await;
    done("her_line_brings_back_what_a_failed_read_missed");
    schema.drop_schema();
}

// ------------------------------------------------------------ with a database
