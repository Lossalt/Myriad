//! G0 scenes of what she has and keeps: what the group told her, what
//! she speaks up from, the words she learns, and what cannot be read.

use super::*;

/// What the group told her about herself: kept once when told again on the
/// screen, the latest three always with her, and an older one the talk
/// comes back to by its words.
#[tokio::test(start_paused = true)]
async fn g0_what_the_group_told_her_comes_with_her() {
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let chat = -77_101;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "what_the_group_told_her_comes_with_her") else {
        return;
    };
    let base = world.base;
    world.members.insert("11".into(), 111);
    site_member(&schema.db, 111, "aming", "阿明").await;
    for (days, told) in [
        (5, "说她说话像一只猫"),
        (4, "说她很会接梗"),
        (3, "说她挺毒舌的"),
        (2, "说她唱歌跑调"),
        (1, "说她爱吃火锅"),
    ] {
        kept_in_venue(&schema.db, &venue, "about_me", told, days, base).await;
    }
    for _ in 0..2 {
        answer(
            &mut world,
            "group_making_sense",
            r#"{"goingOn":"在逗她","theyMean":"问她记不记得","aboutYou":"说她像猫"}"#,
        );
    }
    her_turn(&mut world, 2.0, "我才不像猫");
    her_turn(&mut world, 2.0, "喵");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 你说话像一只猫吗", true)).await;
    wait(120.0).await;
    hear(line(chat, 2, 11, "阿明", "@bot 真的像猫", true)).await;
    wait(120.0).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("what_the_group_told_her_comes_with_her", &world.log);
    schema.drop_schema();
}

/// An older telling the talk comes back to by its meaning, sharing no word
/// with it.
#[tokio::test(start_paused = true)]
async fn g0_what_the_group_told_her_comes_back_by_meaning() {
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let chat = -77_102;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "what_the_group_told_her_comes_back_by_meaning") else {
        return;
    };
    let base = world.base;
    world.members.insert("11".into(), 112);
    site_member(&schema.db, 112, "aming", "阿明").await;
    let told = [
        "说她总在半夜发歌",
        "说她字写得好看",
        "说她打游戏很菜",
        "说她数学不行",
        "说她起床很晚",
        "说她爱看恐怖片",
        "说她讲冷笑话",
        "说她手机没电",
        "说她喜欢蓝色",
        "说她跑步很慢",
        "说她画画不错",
        "说她最近很忙",
    ];
    for (index, told) in told.iter().enumerate() {
        kept_in_venue(
            &schema.db,
            &venue,
            "about_me",
            told,
            20 - index as i64,
            base,
        )
        .await;
        // One older telling is about what the question means; the rest are
        // not.
        let vector = if index == 0 {
            vec![0.0, 1.0, 0.0]
        } else {
            vec![1.0, 0.1 * index as f32, 0.0]
        };
        world.vectors.insert(told.to_string(), vector);
    }
    let asked = "@bot 你是不是夜猫子";
    world
        .vectors
        .insert(asked.to_string(), vec![0.05, 1.0, 0.0]);
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"在问她作息","theyMean":"觉得她熬夜","aboutYou":null}"#,
    );
    her_turn(&mut world, 2.0, "有时候吧");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", asked, true)).await;
    wait(120.0).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("what_the_group_told_her_comes_back_by_meaning", &world.log);
    schema.drop_schema();
}

/// Nobody calls her: what she has that the talk touches (a view of hers,
/// something she heard, a word she knows, a song she played and would tell
/// someone about) is what her judgment of speaking up draws on.
#[tokio::test(start_paused = true)]
async fn g0_she_speaks_up_from_what_she_has() {
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let chat = -77_103;
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "she_speaks_up_from_what_she_has") else {
        return;
    };
    let base = world.base;
    world.members.insert("11".into(), 113);
    site_member(&schema.db, 113, "aming", "阿明").await;
    let db = &schema.db;
    kept_as_hers(
        db,
        "view",
        "很辣但是很香，值得排队",
        r#"{"about":"那家火锅"}"#,
        30,
        base,
    )
    .await;
    kept_as_hers(
        db,
        "view",
        "周杰伦的晴天越听越好",
        r#"{"about":"晴天","was":"一般般"}"#,
        60,
        base,
    )
    .await;
    kept_as_hers(db, "heard", "听说那家火锅周末要排队两小时", "{}", 20, base).await;
    kept_as_hers(
        db,
        "meme",
        &myriad_merope::memes::kept_line("yyds", "形容东西特别好"),
        "{}",
        40,
        base,
    )
    .await;
    let song = r#"{"key":"song:netease:1","thing":{"kind":"song","id":"1","source":"netease","name":"晴天","artist":"周杰伦","album":"","cover":"","durationMs":1},"heard":"Length 4:29.","reaction":"liked","tell":true}"#;
    kept_as_hers(db, "doing", "副歌一出来就想跟着唱", song, 2, base).await;
    answer(
        &mut world,
        "group_chime",
        r#"{"speak":"share","about":"她也爱听晴天","basis":1}"#,
    );
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"在聊火锅和歌","theyMean":"推荐晴天","aboutYou":null}"#,
    );
    her_turn(&mut world, 2.0, "晴天我最近也在循环");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "周五吃那家火锅吧 yyds", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 11, "阿明", "吃完去唱晴天", false)).await;
    wait(900.0).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("she_speaks_up_from_what_she_has", &world.log);
    schema.drop_schema();
}

/// Words in the talk she is not sure of, named by her judgment: one she
/// knows already is left; one the dictionaries have is made into what it
/// means and kept as hers; one a search turns up but that does not say what
/// it means here, and one nothing turns up for, she watches as maybe the
/// group's own. Learned once, a word is known the next time.
#[tokio::test(start_paused = true)]
async fn g0_she_learns_words_from_the_talk() {
    let chat = -77_104;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "she_learns_words_from_the_talk") else {
        return;
    };
    let base = world.base;
    world.members.insert("11".into(), 101);
    kept_as_hers(
        &schema.db,
        "meme",
        &myriad_merope::memes::kept_line("yyds", "形容东西特别好"),
        "{}",
        40,
        base,
    )
    .await;
    world.defined.insert(
        "烧糊了吧唧".into(),
        (
            "烧糊了吧唧".into(),
            "网络用语，形容东西烧焦了还硬说好吃".into(),
            "https://dictionary.example/烧糊了吧唧".into(),
        ),
    );
    world.found.insert(
        myriad_merope::memes::search_query("绝绝子乙"),
        (
            "- 绝绝子乙 (https://search.example/1): 一家公司的名字".into(),
            "https://search.example/1".into(),
        ),
    );
    answer(
        &mut world,
        "group_chime",
        r#"{"speak":"none","about":"","basis":null,"unsure":["yyds","「烧糊了吧唧」","绝绝子乙"]}"#,
    );
    answer(
        &mut world,
        "meme",
        r#"{"meaning":"形容东西烧焦了，还硬说好吃"}"#,
    );
    answer(&mut world, "meme", r#"{"meaning":null}"#);
    answer(
        &mut world,
        "group_chime",
        r#"{"speak":"none","about":"","basis":null,"unsure":["烧糊了吧唧","查无此梗"]}"#,
    );
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "锅又烧糊了吧唧 yyds", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 12, "小红", "这家绝绝子乙也不错", false)).await;
    wait(700.0).await;
    hear(line(chat, 3, 11, "阿明", "查无此梗是什么", false)).await;
    wait(700.0).await;
    // What she knows now, and the words she watches there.
    let known: Vec<(String, Option<String>)> =
        crate::services::agent::memory::unified::own_rows(&schema.db, "meme", 50)
            .await
            .unwrap()
            .into_iter()
            .map(|row| (row.content, row.evidence))
            .collect();
    let watched = crate::services::runtime_registry::get::<serde_json::Value>(
        &schema.db,
        "merope_jargon_watched",
        &venue,
    )
    .await
    .unwrap();
    g0::note("known", json!(known));
    g0::note("watched", json!(watched));
    let world = g0::leave().expect("the world of the scene");
    g0::check("she_learns_words_from_the_talk", &world.log);
    schema.drop_schema();
}

/// Nobody calls her, and what she has to draw on cannot be read (the
/// database is down for it): she leaves that glance, saying nothing. Back,
/// the next glance goes as ever.
#[tokio::test(start_paused = true)]
async fn g0_she_says_nothing_when_what_she_has_cannot_be_read() {
    let chat = -77_106;
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut world = World::new(0.5);
    let Some(schema) = database(
        &mut world,
        "she_says_nothing_when_what_she_has_cannot_be_read",
    ) else {
        return;
    };
    world.members.insert("11".into(), 71061);
    site_member(&schema.db, 71061, "u71061", "阿明").await;
    world.failing.insert(("down".into(), "database".into()), 1);
    answer(
        &mut world,
        "group_chime",
        r#"{"speak":"know","about":"那家店周五人多","basis":null}"#,
    );
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"阿明在约周五去吃那家火锅","theyMean":"在问大家去不去","aboutYou":null}"#,
    );
    her_turn(&mut world, 3.0, "周五那家人多，早点去");
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "周五吃那家火锅吧", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 11, "阿明", "大家觉得呢", false)).await;
    wait(700.0).await;
    hear(line(chat, 3, 11, "阿明", "我也想去那家", false)).await;
    wait(1200.0).await;
    done("she_says_nothing_when_what_she_has_cannot_be_read");
    schema.drop_schema();
}
