//! G0 scenes: the site's group behaviour, played on paused time against a
//! recorded world (see `g0`). Each scene's log is compared with its golden
//! in `g0/`; `MYRIAD_RECORD_G0=1` writes them. A scene uses its own chat,
//! so scenes running side by side never share a group.

use super::*;
use g0::World;
use myriad_agent_rules::onebot::decode::Muted;
use serde_json::json;

const TOKEN: &str = "g0-token";

fn line(chat: i64, id: i64, from: i64, name: &str, text: &str, addressed: bool) -> GroupLine {
    GroupLine::from(TelegramGroupMessage {
        update_id: id,
        message_id: id,
        chat_id: chat,
        message_thread_id: None,
        from_id: from,
        display_name: name.into(),
        text: text.into(),
        addressed,
        reply_to: None,
        images: Vec::new(),
    })
}

/// A line heard as a chat app hands it in: kept in mind, then answered
/// when it calls her and otherwise read with the rest of the talk.
async fn hear(line: GroupLine) {
    if !record(&line).await {
        return;
    }
    if line.addressed {
        tokio::spawn(handle(line, TOKEN.to_string()));
    } else {
        notice(line, TOKEN.to_string());
    }
}

async fn wait(seconds: f64) {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
}

fn answer(world: &mut World, operation: &str, raw: &str) {
    world
        .models
        .entry(operation.to_string())
        .or_default()
        .push_back(Some(raw.to_string()));
}

fn done(name: &str) {
    let world = g0::leave().expect("the world of the scene");
    g0::check(name, &world.log);
}

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

/// A scene's own database: a fresh schema in the throwaway database
/// `MYRIAD_G0_DATABASE_URL` names (never a shared one's default schema).
/// None, and the scene is skipped, when it names none.
fn database(world: &mut World, name: &str) -> Option<g0::Throwaway> {
    let Ok(url) = std::env::var("MYRIAD_G0_DATABASE_URL") else {
        eprintln!("{name}: skipped, MYRIAD_G0_DATABASE_URL names no throwaway database");
        return None;
    };
    let throwaway = g0::Throwaway::open(&url);
    world.db = Some(throwaway.db.clone());
    Some(throwaway)
}

/// Keep `content` in the group `venue` as `source`, said `days` before the
/// scene's start (and last read then).
async fn kept_in_venue(
    db: &sea_orm::DatabaseConnection,
    venue: &str,
    source: &'static str,
    content: &str,
    days: i64,
    base: chrono::DateTime<chrono::Utc>,
) {
    use sea_orm::ConnectionTrait;
    let id = crate::services::agent::memory::unified::remember_in_venue(
        db,
        &format!("group:{venue}"),
        content,
        "{}",
        source,
    )
    .await
    .unwrap()
    .unwrap();
    let at = (base - chrono::Duration::days(days)).fixed_offset();
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, updated_at = $1, valid_from = $1 WHERE id = $2",
        [at.into(), id.into()],
    ))
    .await
    .unwrap();
}

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

/// Keep `content` as hers (`source`), with `evidence`, `hours` before the
/// scene's start.
async fn kept_as_hers(
    db: &sea_orm::DatabaseConnection,
    source: &'static str,
    content: &str,
    evidence: &str,
    hours: i64,
    base: chrono::DateTime<chrono::Utc>,
) {
    use sea_orm::ConnectionTrait;
    let id = crate::services::agent::memory::unified::remember_own(
        db,
        content,
        evidence,
        Vec::new(),
        source,
    )
    .await
    .unwrap()
    .unwrap();
    let at = (base - chrono::Duration::hours(hours)).fixed_offset();
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, updated_at = $1, valid_from = $1 WHERE id = $2",
        [at.into(), id.into()],
    ))
    .await
    .unwrap();
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

/// Keep `content` in the group `venue` (stored form) as `source`, with
/// `evidence`, `hours` before the scene's start.
async fn kept_in_group(
    db: &sea_orm::DatabaseConnection,
    stored: &str,
    source: &'static str,
    content: &str,
    evidence: &str,
    hours: i64,
    base: chrono::DateTime<chrono::Utc>,
) {
    use sea_orm::ConnectionTrait;
    let id = crate::services::agent::memory::unified::remember_in_venue(
        db, stored, content, evidence, source,
    )
    .await
    .unwrap()
    .unwrap();
    let at = (base - chrono::Duration::hours(hours)).fixed_offset();
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, updated_at = $1, valid_from = $1 WHERE id = $2",
        [at.into(), id.into()],
    ))
    .await
    .unwrap();
}

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

/// Someone in a group says, as she would answer them from outside.
fn said(world: &mut World, seconds: f64, text: &str) {
    world
        .says
        .entry("group_stranger".into())
        .or_default()
        .push_back((seconds, Ok(text.to_string())));
}

/// The turtle soup a group's table has on, as kept, and how a puzzle of
/// hers went at the tables it was tried at (when they were left out).
async fn soup_kept(db: &sea_orm::DatabaseConnection, venue: &str) {
    let on = crate::services::runtime_registry::get::<serde_json::Value>(
        db,
        "merope_soup",
        &format!("g:{venue}"),
    )
    .await
    .unwrap()
    .map(|mut game| {
        // When it began is the clock's, not the game's.
        if let Some(game) = game.as_object_mut() {
            game.remove("started");
        }
        game
    });
    g0::note("soupOn", json!(on));
    let made: Vec<serde_json::Value> =
        crate::services::agent::memory::unified::own_rows(db, "made_soup", 10)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|row| serde_json::from_str(row.evidence.as_deref()?).ok())
            .collect();
    g0::note("made", json!(made));
}

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

/// What her turns with a member kept in their group session: who said what.
async fn session_kept(db: &sea_orm::DatabaseConnection, user_id: i32) {
    use sea_orm::ConnectionTrait;
    let rows = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT m.role, m.content FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 ORDER BY m.created_at, m.id",
            [user_id.into()],
        ))
        .await
        .unwrap();
    let kept: Vec<(String, String)> = rows
        .iter()
        .map(|row| {
            (
                row.try_get::<String>("", "role").unwrap_or_default(),
                row.try_get::<String>("", "content").unwrap_or_default(),
            )
        })
        .collect();
    g0::note("session", json!(kept));
    // And what her memory work has waiting from them.
    let jobs = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT payload->'data' AS data FROM runtime_registry WHERE namespace = 'merope_memory_jobs' ORDER BY record_id",
        ))
        .await
        .unwrap();
    let jobs: Vec<serde_json::Value> = jobs
        .iter()
        .filter_map(|row| row.try_get::<serde_json::Value>("", "data").ok())
        .collect();
    g0::note("memoryJobs", json!(jobs));
}

/// Her turns with members run on the site's own run lanes, which every
/// scene in the process shares: scenes with such turns take them one at a
/// time, so one scene's turn never waits on another's (on the wall clock,
/// while the scene's clock runs on).
static WHOLE_TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// What she says in her turn with a member, after the time it takes.
fn her_turn(world: &mut World, seconds: f64, text: &str) {
    world
        .says
        .entry("chat".into())
        .or_default()
        .push_back((seconds, Ok(text.to_string())));
}

/// A member of the site, who may talk to her (the site's permission policy).
async fn site_member(db: &sea_orm::DatabaseConnection, user_id: i32, username: &str, name: &str) {
    use sea_orm::ConnectionTrait;
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO users (id, username, display_name) VALUES ($1, $2, $3)",
        [user_id.into(), username.into(), name.into()],
    ))
    .await
    .unwrap();
    db.execute_raw(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO configurations (key, value) VALUES ('user_perm_ai_chat', 'true'::jsonb) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    ))
    .await
    .unwrap();
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
