//! Running a turn: a member's turn under their account, or a light answer to anyone else.

use super::*;

/// Answer someone from outside the community, with little context, on the
/// site owner's budget. `why` is why she speaks when they did not call her.
pub(super) async fn answer_stranger(
    db: &DatabaseConnection,
    message: &GroupLine,
    token: &str,
    why: Option<&str>,
) -> bool {
    let venue = message.venue();
    let within = with_group(&venue, |group| {
        count_today(&mut group.stranger_replies, STRANGER_REPLIES_PER_DAY)
    })
    .unwrap_or(false);
    if !within {
        info!(%venue, "[Group] enough replies to outsiders today");
        return false;
    }
    let Some(owner) = owner(db).await else {
        return false;
    };
    let stranger = crate::services::agent::merope::group::strangers::Stranger {
        who: format!("{}:{}", message.platform.slug(), message.from),
        name: message.display_name.chars().take(40).collect(),
    };
    let read = reading(message);
    let mut sections: Vec<String> = [
        room(&venue).map(|room| myriad_merope::talk_shape::describe(&room, "How people type here")),
        how_she_differs(&venue).await,
        make_sense(db, message, &read).await,
    ]
    .into_iter()
    .flatten()
    .collect();
    send_typing(message, token).await;
    let began = Instant::now();
    // Words and memes in the talk that she looked up before.
    let talk: String = read
        .transcript
        .iter()
        .rev()
        .take(12)
        .map(|line| line.content.clone())
        .chain(std::iter::once(read.said.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    // (A recording has no database to read them from.)
    #[cfg(test)]
    let look_up = !g0::on();
    #[cfg(not(test))]
    let look_up = true;
    if look_up {
        sections.extend(crate::services::agent::merope::group::memes::section_for(db, &talk).await);
        // And the groups' own words in it.
        sections.extend(
            crate::services::agent::merope::group::jargon::section_for(db, &venue, &talk).await,
        );
    }
    #[cfg(test)]
    let g0_reply = if g0::on() {
        Some(
            tokio::time::timeout(
                TURN_DEADLINE,
                g0::stranger(serde_json::json!({
                    "owner": owner,
                    "venue": venue,
                    "who": stranger.who,
                    "name": stranger.name,
                    "transcript": as_json(&read.transcript),
                    "said": read.said,
                    "why": why,
                    "sections": sections,
                })),
            )
            .await,
        )
    } else {
        None
    };
    #[cfg(not(test))]
    let g0_reply = None;
    let Ok(Some((reply, sticker))) = (if let Some(reply) = g0_reply {
        reply
    } else {
        tokio::time::timeout(
            TURN_DEADLINE,
            crate::services::agent::merope::group::strangers::reply(
                db,
                owner,
                &venue,
                &stranger,
                &read.transcript,
                &read.said,
                why,
                &sections,
            ),
        )
        .await
    }) else {
        return false;
    };
    let reply = without_reply_mark(&reply);
    let sent = say_and_send(message, token, &reply, sticker, began, owner).await;
    if sent {
        answered(message);
        #[cfg(test)]
        if g0::on() {
            g0::note(
                "strangerAfter",
                serde_json::json!({ "owner": owner, "venue": venue, "who": stranger.who, "said": read.said, "reply": reply, "messageId": message.message_id }),
            );
            return sent;
        }
        crate::services::agent::merope::group::strangers::enqueue_after(
            db,
            owner,
            venue,
            stranger,
            read.said,
            reply,
            &message.message_id,
        )
        .await;
    }
    sent
}

pub(super) async fn run_turn(
    db: &DatabaseConnection,
    message: &GroupLine,
    user_id: i32,
    token: &str,
    chime: Option<String>,
    first: bool,
) -> Option<(String, Option<serde_json::Value>)> {
    #[cfg(test)]
    if g0::on() {
        return g0_turn(db, message, user_id, chime, first).await;
    }
    crate::services::principal::current_roles(db, user_id)
        .await
        .ok()??;
    let venue = message.venue();
    let key = (venue.clone(), user_id);
    let mut known = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&key).cloned());
    // After a restart: the one they talked to her in here before, not a
    // new one each time.
    if known.is_none() {
        known = last_group_session(db, user_id, &venue).await;
    }
    // Made as the group's from the start: never read back as a private one.
    let session_id = crate::services::agent::sessions::ensure_session_in(
        db,
        known.as_deref(),
        user_id,
        AgentInteractionMode::Chat,
        Some(&venue),
    )
    .await
    .ok()?;
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(key, session_id.clone());
    }
    // Saying something first, there is no line of theirs to read up to, to
    // make sense of, or to be late for.
    let read = (!first).then(|| reading(message));
    let making_sense = match &read {
        Some(read) => make_sense(db, message, read).await,
        None => None,
    };
    let (transcript, input) = match read {
        Some(read) => (read.transcript, read.said),
        None => (transcript(&venue, None), said_now(message)),
    };
    let run = crate::services::agent::run::start_for_user(
        db.clone(),
        user_id,
        crate::services::agent::run::ProcessRequest {
            input,
            context: Some(crate::services::agent::run::ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(session_id),
                group: Some(crate::services::agent::run::GroupTurn {
                    transcript,
                    room: room(&venue),
                    differs: how_she_differs(&venue).await,
                    making_sense,
                    late: if first { None } else { seen_late(message) },
                    venue,
                    chime,
                    speaker: message.display_name.clone(),
                    first,
                }),
                ..Default::default()
            }),
        },
    )
    .await
    .ok()?;
    let mut events = Box::pin(crate::services::agent::run::agent_run_envelopes(run));
    let mut typing = tokio::time::interval(TYPING_EVERY);
    let deadline = tokio::time::sleep(TURN_DEADLINE);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => {
                warn!(venue = %message.venue(), "[Group] no reply in time; she says nothing");
                return None;
            }
            _ = typing.tick() => send_typing(message, token).await,
            envelope = events.next() => {
                match envelope?.event {
                    AgentProgressEvent::TaskCompleted { success, response, .. } => {
                        // A superseded or failed turn says nothing in the group.
                        if !success {
                            info!(venue = %message.venue(), "[Group] turn superseded or failed; she says nothing");
                            return None;
                        }
                        let text = response
                            .get("message")
                            .and_then(|value| value.as_str())
                            .map(str::trim)
                            .unwrap_or_default()
                            .to_string();
                        let sticker = response.pointer("/data/sticker").cloned();
                        return (!text.is_empty() || sticker.is_some()).then_some((text, sticker));
                    }
                    AgentProgressEvent::Error { .. } => {
                        warn!(venue = %message.venue(), "[Group] her turn failed; she says nothing");
                        return None;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// The conversation as its lines, for a recording.
#[cfg(test)]
pub(super) fn as_json(lines: &[ConversationMessage]) -> Vec<serde_json::Value> {
    lines
        .iter()
        .map(|line| serde_json::json!({ "role": line.role, "content": line.content }))
        .collect()
}

/// A member's turn as `run_turn` puts it together, given to the recording
/// instead of a Chat run.
#[cfg(test)]
async fn g0_turn(
    db: &DatabaseConnection,
    message: &GroupLine,
    user_id: i32,
    chime: Option<String>,
    first: bool,
) -> Option<(String, Option<serde_json::Value>)> {
    let venue = message.venue();
    let read = (!first).then(|| reading(message));
    let making_sense = match &read {
        Some(read) => make_sense(db, message, read).await,
        None => None,
    };
    let (transcript, input) = match read {
        Some(read) => (read.transcript, read.said),
        None => (transcript(&venue, None), said_now(message)),
    };
    let room = room(&venue).map(|room| serde_json::to_value(room).unwrap_or_default());
    let differs = how_she_differs(&venue).await;
    let late = if first { None } else { seen_late(message) };
    let given = serde_json::json!({
        "userId": user_id,
        "input": input,
        "venue": venue,
        "transcript": as_json(&transcript),
        "room": room,
        "differs": differs,
        "makingSense": making_sense,
        "late": late,
        "chime": chime,
        "speaker": message.display_name,
        "first": first,
    });
    let deadline = tokio::time::sleep(TURN_DEADLINE);
    tokio::pin!(deadline);
    let mut typing = tokio::time::interval(TYPING_EVERY);
    let turn = g0::turn(given);
    tokio::pin!(turn);
    loop {
        tokio::select! {
            _ = &mut deadline => return None,
            _ = typing.tick() => send_typing(message, "").await,
            done = &mut turn => {
                let (text, sticker) = done?;
                let text = text.trim().to_string();
                return (!text.is_empty() || sticker.is_some()).then_some((text, sticker));
            }
        }
    }
}
