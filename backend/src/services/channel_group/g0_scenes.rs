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
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    // Not a multiple of the typing mark's 4 s: a tie there has no order.
    world
        .turns
        .push_back((4.5, "去老地方吧，便宜又好吃".into(), None));
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "周五聚餐吗", false)).await;
    wait(2.0).await;
    hear(line(chat, 2, 12, "小红", "我可以", false)).await;
    wait(2.0).await;
    hear(line(chat, 3, 11, "阿明", "@bot 你推荐哪家", true)).await;
    wait(600.0).await;
    done("a_member_calls_her");
}

/// Called while she is answering: the line waits, and the same person's
/// added line is answered with theirs.
#[tokio::test(start_paused = true)]
async fn g0_lines_wait_while_she_is_busy() {
    let chat = -77_002;
    let mut world = World::new(0.3);
    world.members.insert("11".into(), 101);
    world.members.insert("12".into(), 102);
    world.turns.push_back((20.5, "我先看看".into(), None));
    world.turns.push_back((3.0, "火锅可以".into(), None));
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 周五去哪", true)).await;
    wait(15.0).await;
    hear(line(chat, 2, 12, "小红", "@bot 你吃火锅吗", true)).await;
    wait(1.0).await;
    hear(line(chat, 3, 12, "小红", "要辣的", false)).await;
    wait(900.0).await;
    done("lines_wait_while_she_is_busy");
}

/// Nobody calls her: she glances when she is free, judges as herself, and
/// speaks up; five minutes on it is kept whether anyone took it up.
#[tokio::test(start_paused = true)]
async fn g0_she_speaks_up_unasked() {
    let chat = -77_003;
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
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
    world
        .turns
        .push_back((3.0, "周五那家人多，早点去".into(), None));
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "周五吃那家火锅吧", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 11, "阿明", "大家觉得呢", false)).await;
    wait(1200.0).await;
    done("she_speaks_up_unasked");
}

/// No words, only a reaction on the line.
#[tokio::test(start_paused = true)]
async fn g0_she_reacts_without_words() {
    let chat = -77_004;
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    answer(
        &mut world,
        "group_chime",
        r#"{"speak":"react","about":"😁"}"#,
    );
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "我今天把锅烧糊了哈哈哈", false)).await;
    wait(600.0).await;
    done("she_reacts_without_words");
}

/// Someone from outside the community calls her: answered lightly, on the
/// owner's budget, and the exchange is counted after.
#[tokio::test(start_paused = true)]
async fn g0_an_outsider_calls_her() {
    let chat = -77_005;
    let mut world = World::new(0.7);
    answer(
        &mut world,
        "group_making_sense",
        r#"{"goingOn":"有人第一次来","theyMean":"问她是谁","aboutYou":"说她像机器人"}"#,
    );
    world
        .strangers
        .push_back((2.0, "我是这里的常客啦".into(), None));
    g0::enter(world);
    hear(line(chat, 1, 31, "路人", "这个bot是谁", false)).await;
    wait(1.0).await;
    hear(line(chat, 2, 31, "路人", "@bot 你是机器人吗", true)).await;
    wait(600.0).await;
    done("an_outsider_calls_her");
}

/// The talk moved on while she wrote: she looks again, keeps it back, and
/// answers once more from the newest line.
#[tokio::test(start_paused = true)]
async fn g0_she_keeps_back_what_no_longer_fits() {
    let chat = -77_006;
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    world.members.insert("12".into(), 102);
    world
        .turns
        .push_back((10.0, "九点吧\n老地方见".into(), None));
    world.turns.push_back((2.0, "那就十点".into(), None));
    answer(&mut world, "draft_still", r#"{"send":false}"#);
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 周六几点集合", true)).await;
    wait(9.0).await;
    hear(line(chat, 2, 12, "小红", "刚说了十点", false)).await;
    wait(600.0).await;
    done("she_keeps_back_what_no_longer_fits");
}

/// Several lines in one reply, the rest typed one after another.
#[tokio::test(start_paused = true)]
async fn g0_a_reply_goes_out_as_several_messages() {
    let chat = -77_007;
    let mut world = World::new(0.8);
    world.members.insert("11".into(), 101);
    world
        .turns
        .push_back((2.0, "好耶\n我也想去\n带我一个\n@阿明 几点".into(), None));
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 周六去爬山", true)).await;
    wait(600.0).await;
    done("a_reply_goes_out_as_several_messages");
}

/// Muted, she knows it and says nothing there until it lifts.
#[tokio::test(start_paused = true)]
async fn g0_muted_she_says_nothing() {
    let chat = -77_008;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    world.turns.push_back((2.0, "我又能说话啦".into(), None));
    g0::enter(world);
    hear(line(chat, 1, 12, "管理员", "安静点", false)).await;
    muted(&venue, "12", Muted::For(600), false).await;
    hear(line(chat, 2, 11, "阿明", "@bot 你还在吗", true)).await;
    wait(700.0).await;
    hear(line(chat, 3, 11, "阿明", "@bot 现在呢", true)).await;
    wait(600.0).await;
    done("muted_she_says_nothing");
}

/// One who answers her nonstop, as a bot would: after enough rounds she
/// stops answering them for a while.
#[tokio::test(start_paused = true)]
async fn g0_she_stops_answering_one_who_never_stops() {
    let chat = -77_009;
    let mut world = World::new(0.5);
    for _ in 0..LOOP_ROUNDS + 2 {
        world.strangers.push_back((1.0, "在".into(), None));
    }
    g0::enter(world);
    for id in 1..=(LOOP_ROUNDS as i64 + 2) {
        hear(line(chat, id, 77, "复读机", "@bot 在吗", true)).await;
        wait(25.0).await;
    }
    wait(600.0).await;
    done("she_stops_answering_one_who_never_stops");
}

/// Away from the site she is not in the group; back, she takes up the
/// latest line that called her.
#[tokio::test(start_paused = true)]
async fn g0_away_and_back() {
    let chat = -77_010;
    let mut world = World::new(0.5);
    world.away = true;
    world.members.insert("11".into(), 101);
    world.turns.push_back((2.0, "回来了回来了".into(), None));
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
}

/// After a restart the group is read back as a person scrolls up, and the
/// line that called her and that she never got to is taken up, late.
#[tokio::test(start_paused = true)]
async fn g0_read_back_after_a_restart() {
    let chat = -77_011;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    world.turns.push_back((2.0, "抱歉刚看到".into(), None));
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
}

/// Pictures in the talk: looked at when she reads it, said as she saw
/// them, and one sent again is the group's.
#[tokio::test(start_paused = true)]
async fn g0_pictures_as_she_saw_them() {
    let chat = -77_012;
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    world.seen.insert(
        "telegram:cat".into(),
        myriad_merope::seeing::Seen {
            what: "一只翻白眼的猫".into(),
            says: Some("无语".into()),
            keep: true,
            only_there: false,
        },
    );
    world.turns.push_back((2.0, "这猫好懂你".into(), None));
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
}

/// Enough of the group's talk goes by: what she heard in it is taken in.
#[tokio::test(start_paused = true)]
async fn g0_what_she_heard_is_taken_in() {
    let chat = -77_013;
    let world = World::new(0.5);
    g0::enter(world);
    for id in 0..HEARD_EVERY as i64 {
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
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    world.sendable.insert(chat.to_string());
    world
        .turns
        .push_back((2.0, "今天听到一首好歌，分享给你们".into(), None));
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
}

/// Only a sticker: it goes, and she and the group see she sent it.
#[tokio::test(start_paused = true)]
async fn g0_only_a_sticker() {
    let chat = -77_015;
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    world.turns.push_back((
        2.0,
        String::new(),
        Some(json!({ "number": 2, "meaning": "开心" })),
    ));
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 发个表情", true)).await;
    wait(600.0).await;
    done("only_a_sticker");
}

/// The platform refuses a send: what went out is what she said there.
#[tokio::test(start_paused = true)]
async fn g0_a_send_is_refused() {
    let chat = -77_016;
    let mut world = World::new(0.5);
    world.members.insert("11".into(), 101);
    world.turns.push_back((2.0, "第一句\n第二句".into(), None));
    world.refused.insert(1);
    g0::enter(world);
    hear(line(chat, 1, 11, "阿明", "@bot 说两句", true)).await;
    wait(600.0).await;
    done("a_send_is_refused");
}
