//! Talking to the platforms: sending her reply and the typing mark, and looking up who is paired.

use super::*;

/// Deliver one chunk of her reply in the group, as a reply to `line`, with
/// her `@name`s as the platform's mentions.
pub(super) async fn send_reply(
    line: &GroupLine,
    token: &str,
    pieces: &[myriad_agent_rules::mentions::Piece],
    quoting: bool,
) -> Result<(), ConnectFailureKind> {
    use myriad_agent_rules::mentions;
    #[cfg(test)]
    if g0::on() {
        let pieces: Vec<serde_json::Value> = pieces
            .iter()
            .map(|piece| match piece {
                mentions::Piece::Text(text) => serde_json::json!({ "text": text }),
                mentions::Piece::Mention { id, name } => {
                    serde_json::json!({ "mention": id, "name": name })
                }
            })
            .collect();
        let quoted = quoting.then(|| line.message_id.clone());
        return if g0::send(
            "send",
            serde_json::json!({ "venue": line.venue(), "thread": line.thread, "pieces": pieces, "quoted": quoted }),
        ) {
            Ok(())
        } else {
            Err(ConnectFailureKind::Transient)
        };
    }
    match line.platform {
        ChannelPlatform::Telegram => {
            let Ok(chat) = line.chat.parse() else {
                return Err(ConnectFailureKind::Permanent);
            };
            let quoted = match quoting.then(|| line.message_id.parse()) {
                Some(Ok(message_id)) => Some(message_id),
                Some(Err(_)) => return Err(ConnectFailureKind::Permanent),
                None => None,
            };
            let (text, entities) = mentions::telegram_text_and_entities(pieces);
            crate::services::telegram_bot::send_group_reply(
                token,
                chat,
                &text,
                &entities,
                quoted,
                line.thread,
            )
            .await
        }
        ChannelPlatform::Discord => {
            let (content, users) = mentions::discord_content_and_users(pieces);
            crate::services::discord_bot::send_group_reply(
                token,
                &line.chat,
                &content,
                &users,
                quoting.then_some(line.message_id.as_str()),
            )
            .await
        }
        ChannelPlatform::OneBot => {
            let Some(action) = myriad_agent_rules::onebot::encode::encode_group_message(
                &line.chat,
                &mentions::onebot_segments(pieces),
            ) else {
                return Err(ConnectFailureKind::Permanent);
            };
            match crate::services::onebot_send::send_action(action).await {
                Ok(None) => Ok(()),
                Ok(Some(kind)) => Err(kind),
                Err(_) => Err(ConnectFailureKind::Transient),
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => Err(ConnectFailureKind::Permanent),
    }
}

/// Whisper `text` to `user` on `platform` (in private, where only they see
/// it); whether it reached them. A Discord member is written to in the DM
/// channel opened with them; a Telegram user who never started a chat with
/// her, a Discord member who keeps server DMs closed, a QQ user who is not
/// her friend, cannot be reached (the platform refuses).
pub(super) async fn whisper(token: &str, platform: &str, user: &str, text: &str) -> bool {
    #[cfg(test)]
    let who = format!("{platform}:{user}");
    let reached = match platform_of(platform) {
        Some(ChannelPlatform::Telegram) => {
            #[cfg(test)]
            if g0::on() {
                return g0::outbound(
                    "telegram.sendMessage",
                    serde_json::json!({ "chat_id": user, "text": text }),
                    &who,
                );
            }
            crate::services::telegram_bot::send_message(token, user, text)
                .await
                .is_ok()
        }
        Some(ChannelPlatform::Discord) => {
            #[cfg(test)]
            let opened = if g0::on() {
                g0::note(
                    "discord.openDm",
                    serde_json::json!({ "recipient_id": user }),
                );
                Ok(format!("dm-{user}"))
            } else {
                crate::services::discord_bot::open_dm(token, user).await
            };
            #[cfg(not(test))]
            let opened = crate::services::discord_bot::open_dm(token, user).await;
            match opened {
                Ok(channel) => {
                    #[cfg(test)]
                    if g0::on() {
                        return g0::outbound(
                            "discord.send",
                            serde_json::json!({ "channel": channel, "content": text }),
                            &who,
                        );
                    }
                    crate::services::discord_bot::send_message(token, &channel, text)
                        .await
                        .is_ok()
                }
                Err(_) => false,
            }
        }
        Some(ChannelPlatform::OneBot) => {
            use myriad_agent_rules::onebot::encode;
            match encode::encode_private_message(user, &[encode::encode_text_segment(text)]) {
                Some(action) => {
                    #[cfg(test)]
                    if g0::on() {
                        return g0::outbound("onebot.action", action, &who);
                    }
                    matches!(
                        crate::services::onebot_send::send_action(action).await,
                        Ok(None)
                    )
                }
                None => false,
            }
        }
        _ => false,
    };
    if !reached {
        warn!(platform, "[Group] a whisper did not reach them");
    }
    reached
}

pub(super) async fn send_typing(line: &GroupLine, token: &str) {
    #[cfg(test)]
    if g0::on() {
        g0::send("typing", serde_json::json!({ "venue": line.venue() }));
        return;
    }
    match line.platform {
        ChannelPlatform::Telegram => {
            let _ = crate::services::telegram_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::Discord => {
            let _ = crate::services::discord_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::OneBot => {
            // Group typing is not a documented NapCat action. Private typing
            // uses `set_input_status` with a user id, which a group line has
            // no reason to poke.
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => {}
    }
}

/// Put her reaction (one of `joining::REACTIONS`) on `line`. Whether the
/// platform took it.
pub(super) async fn react(line: &GroupLine, token: &str, emoji: &str) -> bool {
    #[cfg(test)]
    if g0::on() {
        return g0::send(
            "react",
            serde_json::json!({ "venue": line.venue(), "messageId": line.message_id, "emoji": emoji }),
        );
    }
    let done = match line.platform {
        ChannelPlatform::Telegram => match (line.chat.parse(), line.message_id.parse()) {
            (Ok(chat), Ok(message_id)) => {
                crate::services::telegram_bot::set_reaction(token, chat, message_id, emoji).await
            }
            _ => Err(ConnectFailureKind::Permanent),
        },
        ChannelPlatform::Discord => {
            crate::services::discord_bot::add_reaction(token, &line.chat, &line.message_id, emoji)
                .await
        }
        ChannelPlatform::OneBot => {
            match myriad_agent_rules::onebot::encode::encode_set_msg_emoji_like(
                &line.message_id,
                emoji,
            ) {
                Some(action) => match crate::services::onebot_send::send_action(action).await {
                    Ok(None) => Ok(()),
                    Ok(Some(kind)) => Err(kind),
                    Err(_) => Err(ConnectFailureKind::Transient),
                },
                None => Err(ConnectFailureKind::Permanent),
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => Err(ConnectFailureKind::Permanent),
    };
    if let Err(kind) = &done {
        warn!(venue = %line.venue(), ?kind, "[Group] her reaction did not go");
    }
    done.is_ok()
}

/// Send the sticker she chose into the group, making it first if it is new;
/// what it means, once it went.
pub(super) async fn send_sticker(
    message: &GroupLine,
    token: &str,
    chosen: &serde_json::Value,
) -> Option<String> {
    use crate::services::agent::merope::group::stickers;
    #[cfg(test)]
    if g0::on() {
        if chosen.get("make").is_some() {
            send_typing(message, token).await;
        }
        if g0::send(
            "sticker",
            serde_json::json!({ "venue": message.venue(), "chosen": chosen }),
        ) {
            let meaning = chosen
                .get("meaning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("贴纸");
            return Some(meaning.to_string());
        }
        return None;
    }
    let db = db().ok()?;
    if chosen.get("make").is_some() {
        send_typing(message, token).await;
    }
    let sticker = stickers::resolve(&db, chosen, None).await?;
    let (png, _) = stickers::picture(&sticker).await?;
    let prepared = crate::services::sticker_send::prepare(png, message.platform).await?;
    let sent = match message.platform {
        ChannelPlatform::Telegram => crate::services::telegram_bot::send_sticker(
            token,
            &message.chat,
            &prepared.bytes,
            message.thread,
        )
        .await
        .is_ok(),
        ChannelPlatform::Discord => crate::services::discord_bot::send_photo(
            token,
            &message.chat,
            &prepared.bytes,
            prepared.mime,
            None,
        )
        .await
        .is_ok(),
        ChannelPlatform::OneBot => {
            use base64::Engine as _;
            let inline = format!(
                "base64://{}",
                base64::engine::general_purpose::STANDARD.encode(&prepared.bytes)
            );
            match myriad_agent_rules::onebot::encode::encode_group_message(
                &message.chat,
                &[myriad_agent_rules::onebot::encode::encode_image_segment(
                    &inline,
                )],
            ) {
                Some(action) => matches!(
                    crate::services::onebot_send::send_action(action).await,
                    Ok(None)
                ),
                None => false,
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => false,
    };
    if sent {
        stickers::sent(&db, &sticker).await;
        // The group sees she sent it, and so does she.
        Some(sticker.meaning)
    } else {
        warn!(venue = %message.venue(), "[Group] sticker not sent");
        None
    }
}
