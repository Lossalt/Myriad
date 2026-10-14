//! The whole steps her group mind still asks of the site (D2a): who
//! someone is, whether they may talk to her, what she makes of the talk,
//! whether she speaks up, and what she says.

use myriad_merope::group::{self as core, Answer, Here, Looked, Spoken, Target, Who};

use super::*;

/// Look at the pictures in the talk she has not seen yet, as a person
/// reads back over what was sent. Billed to the site's owner.
pub(super) async fn look(venue: &str, pictures: Vec<core::Picture>) -> Vec<Looked> {
    let Ok(db) = db() else {
        return Vec::new();
    };
    let Some(owner) = owner(&db).await else {
        return Vec::new();
    };
    let token = token(venue);
    // A sticker she looks at as she is: whether she would keep it to send.
    let soul = soul().await;
    let (db, soul, token) = (&db, soul.as_str(), token.as_str());
    futures::future::join_all(pictures.into_iter().map(|picture| async move {
        let seen = look_at(db, owner, soul, venue, token, &picture.image).await;
        seen.map(|seen| Looked {
            message_id: picture.message_id,
            index: picture.index,
            seen,
        })
    }))
    .await
    .into_iter()
    .flatten()
    .collect()
}

/// Who wrote a line there, and the site's owner.
pub(super) async fn identify(venue: &str, from: &str) -> Answer {
    let Ok(db) = db() else {
        return Answer::Identified {
            who: Who::Unknown,
            owner: None,
        };
    };
    let Some(line) = venue
        .split_once(':')
        .and_then(|(slug, chat)| {
            line_to(&Target {
                platform: slug.to_string(),
                chat: chat.to_string(),
                thread: None,
                message_id: String::new(),
            })
        })
        .map(|line| GroupLine {
            from: from.to_string(),
            ..line
        })
    else {
        return Answer::Identified {
            who: Who::Unknown,
            owner: None,
        };
    };
    let who = match lookup(&db, &line).await {
        Some(PairingLookup::Paired { user_id }) => {
            let binding = current_binding(&db, &line, user_id).await;
            let bound = binding.is_some();
            if let (Some(binding), Ok(mut bindings)) = (binding, BINDINGS.lock()) {
                bindings.insert((venue.to_string(), user_id), binding);
            }
            Who::Member { user_id, bound }
        }
        Some(_) => Who::Stranger,
        None => Who::Unknown,
    };
    Answer::Identified {
        who,
        owner: owner(&db).await,
    }
}

/// Whether `user_id` may talk to her now, with their session in this group.
pub(super) async fn prepare(venue: &str, user_id: i32) -> bool {
    #[cfg(test)]
    if g0::on() {
        return true;
    }
    let Ok(db) = db() else {
        return false;
    };
    if !matches!(
        crate::services::principal::current_roles(&db, user_id).await,
        Ok(Some(_))
    ) {
        return false;
    }
    let key = (venue.to_string(), user_id);
    let mut known = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&key).cloned());
    // After a restart: the one they talked to her in here before.
    if known.is_none() {
        known = last_group_session(&db, user_id, venue).await;
    }
    // Made as the group's from the start: never read back as a private one.
    let Ok(session_id) = crate::services::agent::sessions::ensure_session_in(
        &db,
        known.as_deref(),
        user_id,
        AgentInteractionMode::Chat,
        Some(venue),
    )
    .await
    else {
        return false;
    };
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(key, session_id);
    }
    true
}

/// The session someone last talked to her in, in this group.
pub(super) async fn last_group_session(
    db: &DatabaseConnection,
    user_id: i32,
    venue: &str,
) -> Option<String> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id FROM agent_sessions WHERE user_id = $1 AND context->>'venue' = $2 \
           AND context->>'mode' = 'chat' ORDER BY last_active_at DESC LIMIT 1",
        [user_id.into(), venue.into()],
    ))
    .await
    .ok()
    .flatten()?
    .try_get::<String>("", "id")
    .ok()
}

/// What a member's turn is given.
pub(super) struct MemberTurn {
    pub(super) user_id: i32,
    pub(super) to: Target,
    pub(super) input: String,
    pub(super) transcript: Vec<Spoken>,
    pub(super) room: Option<myriad_merope::talk_shape::Shape>,
    pub(super) differs: Option<String>,
    pub(super) making_sense: Option<String>,
    pub(super) late: Option<String>,
    pub(super) chime: Option<String>,
    pub(super) speaker: String,
    pub(super) first: bool,
}

pub(super) fn as_conversation(lines: &[Spoken]) -> Vec<ConversationMessage> {
    lines
        .iter()
        .map(|line| ConversationMessage {
            role: line.role.clone(),
            content: line.content.clone(),
            created_at: None,
        })
        .collect()
}

/// A member's turn: a Chat turn under their account, in the group venue.
pub(super) async fn member_turn(venue: &str, turn: MemberTurn) -> Answer {
    let Some(line) = line_to(&turn.to) else {
        return Answer::Turned {
            text: None,
            sticker: None,
            bound: false,
        };
    };
    let token = token(venue);
    let said = run_member_turn(venue, &line, &token, &turn).await;
    // Unpaired or switched off while she was thinking: say nothing.
    let bound = if turn.first {
        true
    } else {
        let binding = BINDINGS
            .lock()
            .ok()
            .and_then(|bindings| bindings.get(&(venue.to_string(), turn.user_id)).cloned());
        match (binding, db()) {
            (Some(binding), Ok(db)) => still_bound(&db, &binding).await,
            _ => false,
        }
    };
    let (text, sticker) = match said {
        Some((text, sticker)) => (Some(text), sticker),
        None => (None, None),
    };
    Answer::Turned {
        text,
        sticker,
        bound,
    }
}

pub(super) async fn run_member_turn(
    venue: &str,
    line: &GroupLine,
    token: &str,
    turn: &MemberTurn,
) -> Option<(String, Option<serde_json::Value>)> {
    #[cfg(test)]
    if g0::on() {
        let given = serde_json::json!({
            "userId": turn.user_id,
            "input": turn.input,
            "venue": venue,
            "transcript": turn.transcript,
            "room": turn.room,
            "differs": turn.differs,
            "makingSense": turn.making_sense,
            "late": turn.late,
            "chime": turn.chime,
            "speaker": turn.speaker,
            "first": turn.first,
        });
        let deadline = tokio::time::sleep(TURN_DEADLINE);
        tokio::pin!(deadline);
        let mut typing = tokio::time::interval(TYPING_EVERY);
        let said = g0::turn(given);
        tokio::pin!(said);
        loop {
            tokio::select! {
                _ = &mut deadline => return None,
                _ = typing.tick() => send_typing(line, token).await,
                done = &mut said => {
                    let (text, sticker) = done?;
                    let text = text.trim().to_string();
                    return (!text.is_empty() || sticker.is_some()).then_some((text, sticker));
                }
            }
        }
    }
    let db = db().ok()?;
    let session_id = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&(venue.to_string(), turn.user_id)).cloned())?;
    let run = crate::services::agent::run::start_for_user(
        db.clone(),
        turn.user_id,
        crate::services::agent::run::ProcessRequest {
            input: turn.input.clone(),
            context: Some(crate::services::agent::run::ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(session_id),
                group: Some(crate::services::agent::run::GroupTurn {
                    transcript: as_conversation(&turn.transcript),
                    room: turn.room.clone(),
                    differs: turn.differs.clone(),
                    making_sense: turn.making_sense.clone(),
                    late: turn.late.clone(),
                    venue: venue.to_string(),
                    chime: turn.chime.clone(),
                    speaker: turn.speaker.clone(),
                    first: turn.first,
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
                warn!(%venue, "[Group] no reply in time; she says nothing");
                return None;
            }
            _ = typing.tick() => send_typing(line, token).await,
            envelope = events.next() => {
                match envelope?.event {
                    AgentProgressEvent::TaskCompleted { success, response, .. } => {
                        // A superseded or failed turn says nothing in the group.
                        if !success {
                            info!(%venue, "[Group] turn superseded or failed; she says nothing");
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
                        warn!(%venue, "[Group] her turn failed; she says nothing");
                        return None;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Her light answer to someone from outside, with little context, on the
/// site owner's budget.
pub(super) async fn stranger_turn(
    venue: &str,
    owner: i32,
    stranger: core::Stranger,
    transcript: Vec<Spoken>,
    said: String,
    why: Option<String>,
    mut sections: Vec<String>,
) -> Answer {
    let nothing = Answer::Turned {
        text: None,
        sticker: None,
        bound: true,
    };
    let Ok(db) = db() else {
        return nothing;
    };
    let transcript = as_conversation(&transcript);
    // Words and memes in the talk that she looked up before.
    let talk: String = transcript
        .iter()
        .rev()
        .take(12)
        .map(|line| line.content.clone())
        .chain(std::iter::once(said.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    // (A recording has no database to read them from.)
    #[cfg(test)]
    let look_up = !g0::on();
    #[cfg(not(test))]
    let look_up = true;
    if look_up {
        sections
            .extend(crate::services::agent::merope::group::memes::section_for(&db, &talk).await);
        // And the groups' own words in it.
        sections.extend(
            crate::services::agent::merope::group::jargon::section_for(&db, venue, &talk).await,
        );
    }
    #[cfg(test)]
    if g0::on() {
        let reply = tokio::time::timeout(
            TURN_DEADLINE,
            g0::stranger(serde_json::json!({
                "owner": owner,
                "venue": venue,
                "who": stranger.who,
                "name": stranger.name,
                "transcript": transcript.iter().map(|line| serde_json::json!({ "role": line.role, "content": line.content })).collect::<Vec<_>>(),
                "said": said,
                "why": why,
                "sections": sections,
            })),
        )
        .await;
        return match reply {
            Ok(Some((text, sticker))) => Answer::Turned {
                text: Some(text),
                sticker,
                bound: true,
            },
            _ => nothing,
        };
    }
    let stranger = crate::services::agent::merope::group::strangers::Stranger {
        who: stranger.who,
        name: stranger.name,
    };
    match tokio::time::timeout(
        TURN_DEADLINE,
        crate::services::agent::merope::group::strangers::reply(
            &db,
            owner,
            venue,
            &stranger,
            &transcript,
            &said,
            why.as_deref(),
            &sections,
        ),
    )
    .await
    {
        Ok(Some((text, sticker))) => Answer::Turned {
            text: Some(text),
            sticker,
            bound: true,
        },
        _ => nothing,
    }
}

/// Whether she says something about the talk, billed to `owner`.
pub(super) async fn judge(
    venue: &str,
    owner: i32,
    conversation: &[String],
    here: &Here,
) -> Option<core::Decision> {
    let db = db().ok()?;
    let (why, reason) = crate::services::agent::merope::group::joining::decide(
        &db,
        owner,
        venue,
        conversation,
        &crate::services::agent::merope::group::joining::Here {
            last_spoke: here.last_spoke.as_deref(),
            late: here.late.as_deref(),
            how_it_went: here.how_it_went.as_deref(),
            share: here.share.as_deref(),
        },
    )
    .await?;
    Some(core::Decision { why, reason })
}
