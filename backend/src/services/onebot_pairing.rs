//! OneBot private-chat pairing. The QQ number is the identity key.

use myriad_agent_rules::channel::{ChannelFileRef, ChannelVoiceRef, InboundC2cText};
use sea_orm::DatabaseConnection;

use crate::services::channel_pairing::{self, PrivateText};
use crate::services::channel_platform::ChannelPlatform;
use crate::services::channel_work::attachments::VoiceAudio;

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

    async fn voice_audio(&self, voice: &ChannelVoiceRef) -> Result<Option<VoiceAudio>, String> {
        record_wav(&voice.source).await
    }

    async fn platform_voice_text(&self, _voice: &ChannelVoiceRef) -> Option<String> {
        ptt_text(&self.inbound.msg_id).await
    }

    async fn file_bytes(&self, file: &ChannelFileRef) -> Result<Option<Vec<u8>>, String> {
        let Some((file_id, file_hash)) =
            myriad_agent_rules::onebot::decode::decode_file_handle(&file.url)
        else {
            return crate::services::channel_work::attachments::fetch_public_file(file).await;
        };
        let Some(url) = private_file_url(&self.inbound.user_openid, file_id, file_hash).await
        else {
            return Ok(None);
        };
        crate::services::channel_work::attachments::fetch_public_file(&ChannelFileRef {
            url,
            ..file.clone()
        })
        .await
    }

    async fn start_work(
        &self,
        db: &DatabaseConnection,
        user_id: i32,
        input: &str,
        session_key: &str,
    ) {
        crate::services::onebot_work::start_paired_work_with_images(
            db,
            user_id,
            &self.inbound.user_openid,
            &self.inbound.user_openid,
            input,
            &self.inbound.images,
            session_key,
            self.temp_group.as_deref(),
        )
        .await;
    }
}

/// `get_record` asking for WAV. SnowLuma transcodes and answers `base64`
/// (needs its ffmpeg; uncertain on a given deployment). When that is refused,
/// the plain `get_record` is asked again for a URL. An answer with only a
/// local `file` path is no audio: that disk is not the site's. A voice left
/// without audio falls back to `fetch_ptt_text`.
async fn record_wav(source: &str) -> Result<Option<VoiceAudio>, String> {
    use myriad_agent_rules::onebot::decode::{RecordAudio, decode_record_audio};
    use myriad_agent_rules::onebot::encode::encode_get_record_wav;
    let Some(action) = encode_get_record_wav(source) else {
        return Ok(None);
    };
    // `asked` is the format the answer is in unless it says otherwise; after
    // the untranscoded retry that is unknown.
    let (data, asked) = match crate::services::onebot_send::call_action(action.clone()).await {
        Ok(data) => (data, "wav"),
        Err(error) => {
            tracing::warn!(%error, "OneBot get_record wav refused; asking without transcode");
            let mut plain = action;
            if let Some(params) = plain["params"].as_object_mut() {
                params.remove("out_format");
            }
            (crate::services::onebot_send::call_action(plain).await?, "")
        }
    };
    match decode_record_audio(&data, asked) {
        Some(RecordAudio::Base64 { data, format }) => {
            use base64::Engine;
            Ok(base64::engine::general_purpose::STANDARD
                .decode(data)
                .ok()
                .map(|bytes| VoiceAudio { bytes, format }))
        }
        Some(RecordAudio::Url { url, format }) => {
            crate::services::channel_work::attachments::fetch_public_voice(&ChannelVoiceRef {
                source: url,
                format,
                said: None,
            })
            .await
        }
        None => Ok(None),
    }
}

/// `get_private_file_url` for a private file whose segment had no URL. Not
/// every implementation has it; a refusal leaves the file only named.
async fn private_file_url(user_id: &str, file_id: &str, file_hash: &str) -> Option<String> {
    let action = myriad_agent_rules::onebot::encode::encode_get_private_file_url(
        user_id, file_id, file_hash,
    )?;
    let data = crate::services::onebot_send::call_action(action)
        .await
        .map_err(|error| tracing::warn!(%error, "OneBot get_private_file_url failed"))
        .ok()?;
    data.get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_string)
}

/// QQ's own voice-to-text for a received voice message (`fetch_ptt_text`).
async fn ptt_text(message_id: &str) -> Option<String> {
    let action = myriad_agent_rules::onebot::encode::encode_fetch_ptt_text(message_id)?;
    let data = crate::services::onebot_send::call_action(action)
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
