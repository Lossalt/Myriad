//! OneBot private-chat pairing. The QQ number is the identity key.

use myriad_agent_rules::channel::{InboundC2cText, compose_channel_input};
use sea_orm::DatabaseConnection;

use crate::services::channel_pairing::{self, PrivateText};
use crate::services::channel_platform::ChannelPlatform;

struct OneBotText {
    inbound: InboundC2cText,
    /// The group of a temp-session message; replies go back through it.
    temp_group: Option<String>,
}

impl PrivateText for OneBotText {
    const PLATFORM: ChannelPlatform = ChannelPlatform::OneBot;

    fn inbound(&self) -> &InboundC2cText {
        &self.inbound
    }

    fn session_chat_id(&self) -> String {
        self.inbound.user_openid.clone()
    }

    async fn reply(&self, _db: &DatabaseConnection, text: &str) {
        let Some(action) = myriad_agent_rules::onebot::encode::plan_private_delivery(
            &self.inbound.user_openid,
            text,
            &[],
        ) else {
            return;
        };
        let action = myriad_agent_rules::onebot::encode::via_temp_session(
            action,
            self.temp_group.as_deref(),
        );
        match crate::services::onebot_send::send_action(action).await {
            Ok(None) => {}
            Ok(Some(_)) => tracing::warn!("OneBot pairing reply refused"),
            Err(error) => tracing::warn!(error = %error, "OneBot pairing reply failed"),
        }
    }

    async fn start_work(
        &self,
        db: &DatabaseConnection,
        user_id: i32,
        input: &str,
        session_key: &str,
    ) {
        // Voices and files are read only now, once the sender is paired:
        // pairing never transcribes or downloads for a stranger.
        let input = if self.inbound.has_attachments() {
            let mut voices = Vec::with_capacity(self.inbound.voices.len());
            for voice in &self.inbound.voices {
                voices.push(voice_text(&voice.source, &self.inbound.msg_id).await);
            }
            let files =
                crate::services::channel_work::attachments::read_files(&self.inbound.files).await;
            compose_channel_input(input, &voices, &files)
        } else {
            input.to_string()
        };
        // Other segments are dropped by the decoder. A paired sender would
        // otherwise start Work with an empty input and no reply.
        if input.trim().is_empty() && self.inbound.images.is_empty() {
            self.reply(
                db,
                "这条消息里没有能读的内容：目前只认文字、图片、语音和文件。",
            )
            .await;
            return;
        }
        crate::services::onebot_work::start_paired_work_with_images(
            db,
            user_id,
            &self.inbound.user_openid,
            &self.inbound.user_openid,
            &input,
            &self.inbound.images,
            session_key,
            self.temp_group.as_deref(),
        )
        .await;
    }
}

/// What one voice said: SnowLuma transcodes it to WAV for the site's speech
/// service; failing that, QQ's own voice-to-text is asked.
async fn voice_text(source: &str, message_id: &str) -> Option<String> {
    use crate::services::onebot_send::call_action;
    use base64::Engine;
    use myriad_agent_rules::onebot::encode::{encode_fetch_ptt_text, encode_get_record_wav};

    if let Some(action) = encode_get_record_wav(source) {
        match call_action(action).await {
            Ok(data) => {
                let audio = data
                    .get("base64")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok());
                if let Some(audio) = audio {
                    if let Some(text) =
                        crate::services::channel_work::attachments::transcribe_voice(audio, "wav")
                            .await
                    {
                        return Some(text);
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "OneBot get_record failed"),
        }
    }
    let data = call_action(encode_fetch_ptt_text(message_id)?)
        .await
        .map_err(|error| tracing::warn!(%error, "OneBot fetch_ptt_text failed"))
        .ok()?;
    data.get("text")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Worker entry for one decoded private message.
pub async fn handle_inbound(inbound: InboundC2cText, temp_group: Option<String>) {
    channel_pairing::handle_private_text(OneBotText {
        inbound,
        temp_group,
    })
    .await;
}
