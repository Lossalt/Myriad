//! G0 scenes of the games at a group's table, and her whole turns with
//! members.

use super::*;

/// A puzzle she made on her own time, brought out when someone in a group
/// wants to play: each question judged against its truth, and when they get
/// it, the group remembers the game and the puzzle how it went.
#[tokio::test(start_paused = true)]
async fn g0_a_soup_of_her_own_is_played_and_solved() {
    let chat = -77_107;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "a_soup_of_her_own_is_played_and_solved") else {
        return;
    };
    let base = world.base;
    let made = json!({
        "surface": "一个人喝了一口海龟汤，然后哭了。",
        "truth": "他曾遇海难，同伴骗他吃的是海龟汤，其实是人肉；今天喝到真的海龟汤，他明白了。",
        "keys": ["曾经遇过海难", "吃过人肉"],
        "presentation": "汤面：一个人喝了一口海龟汤，然后哭了。你们只能问我是或不是的问题。",
        "from": "读到一篇海难的故事: 很难过",
        "tried": []
    })
    .to_string();
    kept_as_hers(
        &schema.db,
        "made_soup",
        "一个人喝了一口海龟汤，然后哭了。",
        &made,
        30,
        base,
    )
    .await;
    said(&mut world, 2.5, "好呀，我这有一个\n[[game:soup]]");
    said(&mut world, 2.5, "是的");
    said(&mut world, 2.5, "猜中了！就是这样");
    answer(
        &mut world,
        "soup_judge",
        r#"{"verdict":"yes","found":[0],"solved":false,"gave_up":false}"#,
    );
    answer(
        &mut world,
        "soup_judge",
        r#"{"verdict":"yes","found":[1],"solved":true,"gave_up":false}"#,
    );
    g0::enter(world);
    hear(line(chat, 1, 31, "路人", "@bot 来玩海龟汤", true)).await;
    wait(300.0).await;
    hear(line(chat, 2, 31, "路人", "@bot 他以前遇到过海难吗", true)).await;
    wait(300.0).await;
    hear(line(chat, 3, 31, "路人", "@bot 他是不是吃过人肉", true)).await;
    wait(300.0).await;
    soup_kept(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("a_soup_of_her_own_is_played_and_solved", &world.log);
    schema.drop_schema();
}

/// A puzzle she makes up when asked: a question judged, then nobody asks for
/// hours and it is put away unfinished, truth and all; asked again she makes
/// a different one, and they give up on it.
#[tokio::test(start_paused = true)]
async fn g0_a_soup_she_makes_up_is_left_then_given_up() {
    let chat = -77_108;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.2);
    let Some(schema) = database(&mut world, "a_soup_she_makes_up_is_left_then_given_up") else {
        return;
    };
    answer(
        &mut world,
        "soup_start",
        r#"{"surface":"电梯停在了十三楼，他却走楼梯下去了。","truth":"他是个侏儒，够不到低楼层的按钮，只能按到十三楼，下雨天带伞才能按到更低的。","keys":["他个子矮","按钮够不到"],"presentation":"汤面：电梯停在十三楼，他却走楼梯下去了。问吧，我只答是或不是。"}"#,
    );
    answer(
        &mut world,
        "soup_start",
        r#"{"surface":"她在婚礼上笑着笑着就走了。","truth":"她是婚礼摄影师，拍完了就走。","keys":["她是摄影师"],"presentation":"汤面：她在婚礼上笑着笑着就走了。来问吧。"}"#,
    );
    answer(
        &mut world,
        "soup_judge",
        r#"{"verdict":"no","found":[],"solved":false,"gave_up":false}"#,
    );
    answer(
        &mut world,
        "soup_judge",
        r#"{"verdict":"not_a_question","found":[],"solved":false,"gave_up":true}"#,
    );
    said(&mut world, 2.5, "我想一个\n[[game:soup]]");
    said(&mut world, 2.5, "不是");
    said(&mut world, 2.5, "在呀");
    said(&mut world, 2.5, "那换一个\n[[game:soup]]");
    said(&mut world, 2.5, "好吧，告诉你");
    g0::enter(world);
    hear(line(chat, 1, 31, "路人", "@bot 玩个海龟汤", true)).await;
    wait(300.0).await;
    hear(line(chat, 2, 31, "路人", "@bot 是人做的吗", true)).await;
    wait(11_000.0).await;
    hear(line(chat, 3, 31, "路人", "@bot 还在吗", true)).await;
    wait(300.0).await;
    hear(line(chat, 4, 31, "路人", "@bot 再来一个", true)).await;
    wait(300.0).await;
    hear(line(chat, 5, 31, "路人", "@bot 不玩了 告诉我答案", true)).await;
    wait(300.0).await;
    soup_kept(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("a_soup_she_makes_up_is_left_then_given_up", &world.log);
    schema.drop_schema();
}

/// A member calls her, in a group with a database of its own: her turn runs
/// as the site runs it (the member's own session in the group, what she
/// knows of them and how she is, the group's own), and what she said is kept
/// in their session.
#[tokio::test(start_paused = true)]
async fn g0_a_member_calls_her_and_her_turn_runs_whole() {
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let chat = -77_109;
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "a_member_calls_her_and_her_turn_runs_whole") else {
        return;
    };
    world.members.insert("11".into(), 101);
    site_member(&schema.db, 101, "aming", "阿明").await;
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"约周五吃饭","theyMean":"问她推荐哪家","aboutYou":null}"#,
    );
    world
        .says
        .entry("chat".into())
        .or_default()
        .push_back((2.5, Ok("去老地方吧\n便宜又好吃".into())));
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "周五聚餐吗", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 11, "阿明", "@bot 你推荐哪家", true)).await;
    wait(600.0).await;
    session_kept(&schema.db, 101).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("a_member_calls_her_and_her_turn_runs_whole", &world.log);
    schema.drop_schema();
}

/// A member asks her for a turtle soup in a group: her turn starts one, each
/// question of theirs is judged, a sticker she kept goes with a reply, and
/// when they give up the group remembers the game.
#[tokio::test(start_paused = true)]
async fn g0_a_member_plays_a_soup_and_she_sends_a_sticker() {
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    use sea_orm::ConnectionTrait;
    let chat = -77_110;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.4);
    let Some(schema) = database(&mut world, "a_member_plays_a_soup_and_she_sends_a_sticker") else {
        return;
    };
    world.members.insert("12".into(), 102);
    site_member(&schema.db, 102, "xiaohong", "小红").await;
    schema
        .db
        .execute_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "INSERT INTO merope_stickers (id, picture, meaning, venue, asset_id, identity, sent, kept_from) \
             VALUES ('sticker-9', '一只摊手的猫', '没办法', NULL, 'asset-9', '', 0, 'telegram:-77199')",
        ))
        .await
        .unwrap();
    world
        .titles
        .insert("telegram:-77199".into(), "隔壁群".into());
    answer(
        &mut world,
        "soup_start",
        r#"{"surface":"他把灯关了，然后很多人死了。","truth":"他是灯塔看守，关了灯，船撞上礁石。","keys":["他是灯塔看守","船撞礁"],"presentation":"汤面：他把灯关了，然后很多人死了。问吧。"}"#,
    );
    answer(
        &mut world,
        "soup_judge",
        r#"{"verdict":"no","found":[],"solved":false,"gave_up":false}"#,
    );
    answer(
        &mut world,
        "soup_judge",
        r#"{"verdict":"not_a_question","found":[],"solved":false,"gave_up":true}"#,
    );
    let says = world.says.entry("chat".into()).or_default();
    says.push_back((2.5, Ok("好呀\n[[game:soup]]".into())));
    says.push_back((2.5, Ok("不是哦\n[[sticker:1]]".into())));
    says.push_back((2.5, Ok("好吧，告诉你".into())));
    g0::enter(world);
    hear(line(chat, 1, 12, "小红", "@bot 来玩海龟汤", true)).await;
    wait(300.0).await;
    hear(line(chat, 2, 12, "小红", "@bot 他是杀手吗", true)).await;
    wait(300.0).await;
    hear(line(chat, 3, 12, "小红", "@bot 放弃了 答案是啥", true)).await;
    wait(300.0).await;
    session_kept(&schema.db, 102).await;
    soup_kept(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("a_member_plays_a_soup_and_she_sends_a_sticker", &world.log);
    schema.drop_schema();
}
