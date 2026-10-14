//! G0 scenes of people from outside the community.

use super::*;

/// Someone from outside calls her, someone she keeps a note on: her reply
/// is put together from who they are to her (her note here, what she noted
/// of them in another group), the hour, the group's own bits and days and
/// another group's, her views and what she heard elsewhere that their words
/// touch, the words and memes she knows and the groups' own, the stickers
/// she can send, and the turtle soup on offer; asked once more when the
/// answer is cut. Her note on them then catches up in her memory work, and
/// she wonders whether they are someone she knows; the next reply says so.
#[tokio::test(start_paused = true)]
async fn g0_an_outsider_she_keeps_a_note_on() {
    use crate::services::agent::memory::unified;
    use sea_orm::ConnectionTrait;
    let chat = -77_105;
    let venue = format!("telegram:{chat}");
    let here = format!("group:{venue}");
    let elsewhere = "group:telegram:-77199";
    let mut world = World::new(0.7);
    let Some(schema) = database(&mut world, "an_outsider_she_keeps_a_note_on") else {
        return;
    };
    let base = world.base;
    let db = &schema.db;
    world.titles.insert(venue.clone(), "爬山小组".into());
    world
        .titles
        .insert("telegram:-77199".into(), "隔壁群".into());
    let stranger = myriad_merope::strangers::Stranger {
        who: "telegram:31".into(),
        name: "路人甲".into(),
    };
    let marker = myriad_merope::strangers::evidence_of(&stranger);
    kept_in_group(db, &here, "stranger", "路人甲常来聊爬山", &marker, 30, base).await;
    kept_in_group(
        db,
        elsewhere,
        "stranger",
        "在隔壁群说自己是夜猫子",
        &marker,
        50,
        base,
    )
    .await;
    for (hours, handle, how) in [
        (1, "山顶见", "谁先到山顶谁请奶茶"),
        (2, "迷路王", "阿明每次都走错路"),
        (3, "早起团", "说好六点集合没人起得来"),
        (40, "七星山", "上次在七星山看了日出"),
    ] {
        let evidence = json!({ "handle": handle }).to_string();
        kept_in_group(db, &here, "bit", how, &evidence, hours, base).await;
    }
    kept_in_group(
        db,
        &here,
        "group_day",
        "大家约了周末爬山",
        r#"{"day":"2026-10-12"}"#,
        48,
        base,
    )
    .await;
    kept_in_group(
        db,
        elsewhere,
        "bit",
        "隔壁群也有人说七星山的日出好看",
        r#"{"handle":"日出党"}"#,
        20,
        base,
    )
    .await;
    kept_in_group(
        db,
        elsewhere,
        "jargon",
        &myriad_merope::memes::kept_line("芝麻糊", "隔壁群里说累坏了"),
        "{}",
        10,
        base,
    )
    .await;
    kept_as_hers(
        db,
        "view",
        "七星山的日出比山顶的风景还好",
        r#"{"about":"七星山"}"#,
        60,
        base,
    )
    .await;
    kept_as_hers(
        db,
        "heard",
        "听说七星山周六人很多",
        r#"{"group":"telegram:-77199"}"#,
        5,
        base,
    )
    .await;
    kept_as_hers(
        db,
        "meme",
        &myriad_merope::memes::kept_line("yyds", "形容东西特别好"),
        "{}",
        40,
        base,
    )
    .await;
    // A sticker she kept from the other group, still in use.
    db.execute_raw(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO merope_stickers (id, picture, meaning, venue, asset_id, identity, sent, kept_from) \
         VALUES ('sticker-1', '一只爬山的猫', '爬山累了', NULL, 'asset-1', '', 0, 'telegram:-77199')",
    ))
    .await
    .unwrap();
    // Someone she knows from the community, whom what they say touches.
    let row = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "INSERT INTO users (username, display_name) VALUES ('xiaolin', '小林') RETURNING id",
        ))
        .await
        .unwrap()
        .unwrap();
    let user_id: i32 = row.try_get("", "id").unwrap();
    for content in ["小林家的猫叫芝麻糊", "小林每周六去七星山看日出"] {
        unified::remember(
            db,
            unified::NewMemory {
                user_id,
                kind: unified::MemoryKind::Fact,
                content: content.into(),
                evidence: None,
                speaker: unified::Speaker::User,
                source: "chat",
                audience: unified::Audience::private(user_id),
                importance: 0.5,
                concepts: Vec::new(),
            },
        )
        .await
        .unwrap();
    }
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"路人甲来聊爬山","theyMean":"问她去不去七星山","aboutYou":null}"#,
    );
    let says = world.says.entry("group_stranger".into()).or_default();
    says.push_back((1.5, Err("cut")));
    says.push_back((2.5, Ok("七星山我也想去\n[[sticker:1]]".into())));
    says.push_back((2.5, Ok("芝麻糊是谁呀".into())));
    answer(
        &mut world,
        "merope_stranger_note",
        r#"{"note":"路人甲常去七星山看日出，家里有只猫叫芝麻糊"}"#,
    );
    answer(
        &mut world,
        "recognizing",
        r#"{"sure":"maybe","why":"他说周六去七星山，家里的猫叫芝麻糊"}"#,
    );
    g0::enter(world);
    hear(line(
        chat,
        1,
        31,
        "路人甲",
        "@bot 周六去七星山看日出吗 yyds",
        true,
    ))
    .await;
    wait(300.0).await;
    // Her memory work catches up with what they said.
    db.execute_raw(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE runtime_registry SET payload = jsonb_set(payload, '{ready}', '0'::jsonb) \
         WHERE namespace = 'merope_memory_jobs'",
    ))
    .await
    .unwrap();
    crate::services::agent::merope::group::memory_jobs::tick(db.clone()).await;
    wait(10.0).await;
    hear(line(chat, 2, 31, "路人甲", "@bot 我家芝麻糊也爱爬山", true)).await;
    wait(300.0).await;
    let world = g0::leave().expect("the world of the scene");
    g0::check("an_outsider_she_keeps_a_note_on", &world.log);
    schema.drop_schema();
}
