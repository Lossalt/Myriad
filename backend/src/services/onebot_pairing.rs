//! OneBot private-chat pairing. The QQ number is the identity key.

use myriad_agent_rules::channel::{ChannelVoiceRef, InboundC2cText};
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

/// SnowLuma transcodes a voice to WAV and answers `data.base64`. The
/// transcode needs its ffmpeg support (uncertain on a given deployment).
async fn record_wav(source: &str) -> Result<Option<VoiceAudio>, String> {
    use base64::Engine;
    let Some(action) = myriad_agent_rules::onebot::encode::encode_get_record_wav(source) else {
        return Ok(None);
    };
    let data = crate::services::onebot_send::call_action(action).await?;
    Ok(data
        .get("base64")
        .and_then(serde_json::Value::as_str)
        .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
        .map(|bytes| VoiceAudio {
            bytes,
            format: "wav".into(),
        }))
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
