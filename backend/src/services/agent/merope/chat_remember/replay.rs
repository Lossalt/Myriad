//! Her real talk, gone over again three ways, to see which keeps what a
//! friend would: line by line as production does now (their line, her
//! reply, her line before it); line by line with the talk so far in view;
//! and a whole stretch of talk at once, once they have gone quiet.
//!
//! Read only (the site's database), and it asks the judgment model once per
//! line or stretch. What each way keeps is its own: nothing is written, and
//! the facts one way keeps never reach another. Counted in code (calls,
//! facts proposed, facts whose evidence is not what they said, facts
//! replaced) and every fact kept is in the report beside the talk it came
//! from, for reading.
//!
//! `MEROPE_REMEMBER_REPLAY_REPORT=<new file> DATABASE_URL=… cargo test -p
//! myriad-backend --bin myriad-backend -- --ignored
//! her_talk_gone_over_three_ways --nocapture`; `MEROPE_REMEMBER_REPLAY_WAYS`
//! a comma list of `line`, `window`, `talk` (default all three).

use std::time::Duration;

use futures::StreamExt;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{Value, json};

use myriad_merope::chat_remember::{
    EXTRACT_SCHEMA_NAME, extract_schema, extract_system_prompt, strip_json_fence,
};

/// Quiet this long, and a stretch of talk is over.
const QUIET: chrono::Duration = chrono::Duration::minutes(3);
/// Talk that never goes quiet is still gone over this often.
const AT_MOST: chrono::Duration = chrono::Duration::minutes(20);
/// Lines of the talk so far shown with each line, in the window way.
const WINDOW: usize = 12;
const LINE_CHARS: usize = 300;
const CALL_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Clone)]
struct Line {
    theirs: bool,
    text: String,
    at: chrono::DateTime<chrono::Utc>,
}

/// One person in one place, every line in order.
struct Stream {
    user_id: i32,
    venue: String,
    lines: Vec<Line>,
}

#[derive(Default)]
struct Tally {
    calls: usize,
    failed: usize,
    proposed: usize,
    not_what_they_said: usize,
    already_known: usize,
    replaced: usize,
}

fn clipped(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(LINE_CHARS)
        .collect()
}

/// Production's prompt with one passage swapped, failing loudly if the
/// passage is no longer there (the comparison would silently drift).
fn swapped(prompt: String, swaps: &[(&str, &str)]) -> String {
    swaps.iter().fold(prompt, |prompt, (old, new)| {
        assert!(prompt.contains(old), "prompt changed: {old}");
        prompt.replacen(old, new, 1)
    })
}

const EVIDENCE_LINE: &str =
    "must be a contiguous verbatim excerpt from userText where they stated it. Do not cite reply.";

/// The window way: their line still, with the talk before it in view.
fn window_prompt(known: &[String]) -> String {
    swapped(
        extract_system_prompt(known),
        &[
            (
                "before is what you said just before their message: use it only to understand what userText answers (a short reply to your question), still taking the fact from userText.",
                "conversation is the talk just before userText, oldest first (who: them or you): use it to understand userText; a short line often completes something they said a moment ago, and the fact may then rest on their earlier lines too. Take facts only from what they stated, never from your lines.",
            ),
            (
                EVIDENCE_LINE,
                "must be verbatim from their words where they stated it (userText or their lines in conversation): one contiguous excerpt, or several from different lines joined with ' / '. Do not cite your lines.",
            ),
        ],
    )
}

/// The talk way: a whole stretch, once it is over.
fn talk_prompt(known: &[String]) -> String {
    swapped(
        extract_system_prompt(known),
        &[
            (
                "from this message of theirs.",
                "from this stretch of talk with them.",
            ),
            (
                "taken only from what they explicitly stated in userText;",
                "taken only from what they explicitly stated in their lines (who: them);",
            ),
            (
                "reply is context only; never treat your guesses as their facts. \
before is what you said just before their message: use it only to understand what userText answers (a short reply to your question), still taking the fact from userText.",
                "conversation is the stretch in order, oldest first; your lines (who: you) are context only, never treat your guesses as their facts. A fact is often spread over several of their short lines; put it together.",
            ),
            (
                EVIDENCE_LINE,
                "must be verbatim from their lines where they stated it: one contiguous excerpt, or several from different lines joined with ' / '. Do not cite your lines.",
            ),
            (
                "with evidence a contiguous verbatim excerpt from reply.",
                "with evidence a contiguous verbatim excerpt from your lines.",
            ),
            (
                "with evidence a contiguous verbatim excerpt from userText.",
                "with evidence a contiguous verbatim excerpt from their lines.",
            ),
            (
                "If inGame is true, userText is a move",
                "If inGame is true, their lines are moves",
            ),
        ],
    )
}

/// Whether `evidence` is what they said: each part (joined with ` / ` when
/// `parts` allows) verbatim in `theirs`.
fn what_they_said(evidence: &str, theirs: &str, parts: bool) -> bool {
    let evidence = evidence.trim();
    if evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2 {
        return false;
    }
    if !parts {
        return theirs.contains(evidence);
    }
    evidence
        .split(" / ")
        .map(str::trim)
        .all(|part| !part.is_empty() && theirs.contains(part))
}

/// Keep what one answer gives: facts whose evidence is what they said, not
/// already known; what it replaces goes.
fn keep(
    raw: &str,
    theirs: &str,
    parts: bool,
    known: &mut Vec<String>,
    kept: &mut Vec<Value>,
    tally: &mut Tally,
    from: &Value,
) {
    let Ok(answer) = serde_json::from_str::<Value>(strip_json_fence(raw)) else {
        tally.failed += 1;
        return;
    };
    for old in answer["supersedes"].as_array().into_iter().flatten() {
        if let Some(old) = old.as_str()
            && let Some(at) = known.iter().position(|fact| fact == old)
        {
            known.remove(at);
            tally.replaced += 1;
            for fact in kept.iter_mut() {
                if fact["fact"] == old {
                    fact["replaced"] = json!(true);
                }
            }
        }
    }
    for item in answer["facts"].as_array().into_iter().flatten() {
        let (Some(fact), Some(evidence)) = (item["fact"].as_str(), item["evidence"].as_str())
        else {
            continue;
        };
        tally.proposed += 1;
        if !what_they_said(evidence, theirs, parts) {
            tally.not_what_they_said += 1;
            kept.push(
                json!({"fact": fact, "evidence": evidence, "rejected": "evidence", "from": from}),
            );
            continue;
        }
        if known.iter().any(|old| old == fact) {
            tally.already_known += 1;
            continue;
        }
        known.push(fact.to_string());
        kept.push(json!({"fact": fact, "evidence": evidence, "from": from}));
    }
}

fn today(at: chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&super::super::clock::zone())
        .format("%Y-%m-%d (%A)")
        .to_string()
}

fn talk_lines(lines: &[Line]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| json!({"who": if line.theirs { "them" } else { "you" }, "text": clipped(&line.text)}))
        .collect()
}

/// Stretches of talk: a new one after a quiet spell, or once one has run
/// long without one.
fn stretches(lines: &[Line]) -> Vec<&[Line]> {
    let mut out = Vec::new();
    let mut start = 0;
    for index in 1..=lines.len() {
        let over = index == lines.len()
            || lines[index].at - lines[index - 1].at > QUIET
            || lines[index].at - lines[start].at > AT_MOST;
        if over {
            out.push(&lines[start..index]);
            start = index;
        }
    }
    out
}

async fn ask(
    analyzer: &crate::services::analyzer::AiAnalyzer,
    system: &str,
    input: &str,
    tally: &mut Tally,
) -> Option<String> {
    tally.calls += 1;
    let answer = tokio::time::timeout(
        CALL_TIMEOUT,
        analyzer.analyze_json(system, input, EXTRACT_SCHEMA_NAME, Some(&extract_schema())),
    )
    .await;
    match answer {
        Ok(Ok(raw)) => Some(raw),
        _ => {
            tally.failed += 1;
            None
        }
    }
}

async fn go_over(
    analyzer: &crate::services::analyzer::AiAnalyzer,
    stream: &Stream,
    way: &str,
) -> (Tally, Vec<Value>) {
    let mut tally = Tally::default();
    let mut known: Vec<String> = Vec::new();
    let mut kept = Vec::new();
    match way {
        "talk" => {
            for stretch in stretches(&stream.lines) {
                let theirs: Vec<&str> = stretch
                    .iter()
                    .filter(|line| line.theirs)
                    .map(|line| line.text.as_str())
                    .collect();
                if theirs.is_empty() {
                    continue;
                }
                let theirs = theirs.join("\n");
                let input = json!({
                    "conversation": talk_lines(stretch),
                    "today": today(stretch[0].at),
                })
                .to_string();
                let from = json!({"at": stretch[0].at, "lines": stretch.len()});
                if let Some(raw) = ask(analyzer, &talk_prompt(&known), &input, &mut tally).await {
                    keep(
                        &raw, &theirs, true, &mut known, &mut kept, &mut tally, &from,
                    );
                }
            }
        }
        _ => {
            for (index, line) in stream.lines.iter().enumerate() {
                if !line.theirs || !super::should_extract_chat_remember(&line.text) {
                    continue;
                }
                let reply = stream.lines[index + 1..]
                    .iter()
                    .take_while(|next| !next.theirs)
                    .map(|next| next.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let before = stream.lines[..index].iter().rev().find(|line| !line.theirs);
                let from = json!({"at": line.at, "line": clipped(&line.text)});
                let (system, input, theirs, parts) = if way == "window" {
                    let earlier = &stream.lines[index.saturating_sub(WINDOW)..index];
                    let theirs = earlier
                        .iter()
                        .chain(std::iter::once(line))
                        .filter(|line| line.theirs)
                        .map(|line| line.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let input = json!({
                        "userText": line.text,
                        "reply": super::super::ingest::compact_summary(&reply),
                        "conversation": talk_lines(earlier),
                        "today": today(line.at),
                    })
                    .to_string();
                    (window_prompt(&known), input, theirs, true)
                } else {
                    let turn = super::super::TurnContext {
                        before: before.map(|line| line.text.clone()),
                        ..Default::default()
                    };
                    let input = super::extract_input(
                        &line.text,
                        &reply,
                        &turn,
                        line.at.with_timezone(&super::super::clock::zone()),
                    );
                    (
                        extract_system_prompt(&known),
                        input,
                        line.text.clone(),
                        false,
                    )
                };
                if let Some(raw) = ask(analyzer, &system, &input, &mut tally).await {
                    keep(
                        &raw, &theirs, parts, &mut known, &mut kept, &mut tally, &from,
                    );
                }
            }
        }
    }
    (tally, kept)
}

#[tokio::test]
#[ignore = "reads the site's database and asks its judgment model"]
async fn her_talk_gone_over_three_ways() {
    let db = crate::services::agent::semantic_eval::load_configured_lite().await;
    let ways: Vec<String> = std::env::var("MEROPE_REMEMBER_REPLAY_WAYS")
        .unwrap_or_else(|_| "line,window,talk".into())
        .split(',')
        .map(|way| way.trim().to_string())
        .filter(|way| matches!(way.as_str(), "line" | "window" | "talk"))
        .collect();
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT s.user_id, COALESCE(s.context->>'venue', 'private') AS venue, m.role, \
               m.content, m.created_at \
             FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.context->>'mode' = 'chat' AND s.user_id > 0 \
               AND m.role IN ('user', 'assistant') \
             ORDER BY s.user_id, venue, m.created_at",
        ))
        .await
        .unwrap();
    let mut streams: Vec<Stream> = Vec::new();
    for row in &rows {
        let user_id: i32 = row.try_get("", "user_id").unwrap();
        let venue: String = row.try_get("", "venue").unwrap();
        let line = Line {
            theirs: row.try_get::<String>("", "role").unwrap() == "user",
            text: row.try_get("", "content").unwrap(),
            at: row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .unwrap()
                .with_timezone(&chrono::Utc),
        };
        match streams.last_mut() {
            Some(stream) if stream.user_id == user_id && stream.venue == venue => {
                stream.lines.push(line)
            }
            _ => streams.push(Stream {
                user_id,
                venue,
                lines: vec![line],
            }),
        }
    }
    let analyzer =
        crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(CALL_TIMEOUT))
            .await
            .expect("judgment model");
    let model = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_lite_judge_ai_config()
        .map(|resolved| resolved.model);
    let mut report = json!({"model": model, "ways": {}, "streams": []});
    let mut per_stream: Vec<Value> = streams
        .iter()
        .map(|stream| {
            json!({
                "userId": stream.user_id,
                "venue": stream.venue,
                "theirLines": stream.lines.iter().filter(|line| line.theirs).count(),
                "talk": stream.lines.iter().map(|line| json!({
                    "who": if line.theirs { "them" } else { "her" },
                    "at": line.at,
                    "text": clipped(&line.text),
                })).collect::<Vec<_>>(),
                "kept": {},
            })
        })
        .collect();
    for way in &ways {
        let results: Vec<(Tally, Vec<Value>)> = futures::stream::iter(streams.iter())
            .map(|stream| go_over(&analyzer, stream, way))
            .buffered(4)
            .collect()
            .await;
        let mut total = Tally::default();
        let mut facts = 0;
        for (index, (tally, kept)) in results.into_iter().enumerate() {
            total.calls += tally.calls;
            total.failed += tally.failed;
            total.proposed += tally.proposed;
            total.not_what_they_said += tally.not_what_they_said;
            total.already_known += tally.already_known;
            total.replaced += tally.replaced;
            facts += kept
                .iter()
                .filter(|fact| fact.get("rejected").is_none() && fact.get("replaced").is_none())
                .count();
            per_stream[index]["kept"][way] = json!(kept);
        }
        let line = json!({
            "calls": total.calls,
            "failed": total.failed,
            "proposed": total.proposed,
            "notWhatTheySaid": total.not_what_they_said,
            "alreadyKnown": total.already_known,
            "replaced": total.replaced,
            "keptAtTheEnd": facts,
        });
        println!("{way}: {line}");
        report["ways"][way] = line;
    }
    report["streams"] = json!(per_stream);
    if let Ok(path) = std::env::var("MEROPE_REMEMBER_REPLAY_REPORT") {
        assert!(
            !std::path::Path::new(&path).exists(),
            "report exists: {path}"
        );
        std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        println!("report: {path}");
    }
}

#[test]
fn both_ways_still_swap_into_productions_prompt() {
    let known = vec!["喜欢咖啡".to_string()];
    for prompt in [window_prompt(&known), talk_prompt(&known)] {
        assert!(prompt.contains("' / '"));
        assert!(prompt.contains("- 喜欢咖啡"));
    }
    assert!(talk_prompt(&[]).contains("from this stretch of talk with them."));
}

#[test]
fn evidence_is_what_they_said_in_one_piece_or_several() {
    let theirs = "明天考试\n数学\n好紧张";
    assert!(what_they_said("明天考试", theirs, false));
    assert!(!what_they_said("明天考试 / 数学", theirs, false));
    assert!(what_they_said("明天考试 / 数学", theirs, true));
    assert!(!what_they_said("明天考物理", theirs, true));
    assert!(!what_they_said("明天考试 / ", theirs, true));
}
