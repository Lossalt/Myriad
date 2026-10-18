//! Voices and files of a private message, read into text before Work starts.
//! Each platform only fetches bytes behind its own handles
//! ([`PrivateText::voice_audio`], [`PrivateText::file_bytes`]); transcribing,
//! deciding what is read, and composing the input are shared.
//!
//! [`PrivateText::voice_audio`]: crate::services::channel_pairing::PrivateText::voice_audio
//! [`PrivateText::file_bytes`]: crate::services::channel_pairing::PrivateText::file_bytes

use std::time::Duration;

use myriad_agent_rules::channel::{
    CHANNEL_FILE_LIMIT, CHANNEL_TEXT_FILE_BYTES, CHANNEL_VOICE_BYTES, CHANNEL_VOICE_LIMIT,
    ChannelFileRead, ChannelFileRef, ChannelVoiceRef, InboundC2cText, channel_file_is_text,
    compose_channel_input,
};
use tracing::warn;

use crate::services::channel_pairing::PrivateText;

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Audio fetched for a voice, and the container the speech service is told.
pub(crate) struct VoiceAudio {
    pub bytes: Vec<u8>,
    pub format: String,
}

/// The text Work starts with: what they typed, then what each voice said,
/// then each file. Called only once the sender is paired; a stranger's
/// voices and files are never fetched.
pub(crate) async fn compose_input<T: PrivateText>(text: &T, typed: &str) -> String {
    let inbound: &InboundC2cText = text.inbound();
    if !inbound.has_attachments() {
        return typed.to_string();
    }
    let mut voices = Vec::new();
    for voice in inbound.voices.iter().take(CHANNEL_VOICE_LIMIT) {
        voices.push(voice_said(text, voice).await);
    }
    let mut files = Vec::new();
    for file in inbound.files.iter().take(CHANNEL_FILE_LIMIT) {
        files.push((file.clone(), read_file(text, file).await));
    }
    compose_channel_input(typed, &voices, &files)
}

async fn voice_said<T: PrivateText>(text: &T, voice: &ChannelVoiceRef) -> Option<String> {
    let audio = match text.voice_audio(voice).await {
        Ok(audio) => audio,
        Err(error) => {
            warn!(platform = %T::PLATFORM, %error, "channel voice fetch failed");
            None
        }
    };
    if let Some(audio) = audio
        && let Some(said) = transcribe(audio).await
    {
        return Some(said);
    }
    if let Some(said) = voice.said.as_deref().map(str::trim)
        && !said.is_empty()
    {
        return Some(said.to_string());
    }
    text.platform_voice_text(voice).await
}

async fn transcribe(audio: VoiceAudio) -> Option<String> {
    if audio.bytes.is_empty() || audio.bytes.len() > CHANNEL_VOICE_BYTES {
        return None;
    }
    let format = if audio.format.is_empty() {
        "ogg"
    } else {
        audio.format.as_str()
    };
    match crate::services::speech_runtime::transcribe_bytes(audio.bytes, format, Some("zh")).await {
        Ok(said) => Some(said.trim().to_string()).filter(|said| !said.is_empty()),
        Err(error) => {
            warn!(%error, "channel voice transcription failed");
            None
        }
    }
}

async fn read_file<T: PrivateText>(text: &T, file: &ChannelFileRef) -> ChannelFileRead {
    if file.url.is_empty() || !channel_file_is_text(&file.name, file.size) {
        return ChannelFileRead::Named;
    }
    match text.file_bytes(file).await {
        Ok(Some(bytes)) if bytes.len() as u64 <= CHANNEL_TEXT_FILE_BYTES => {
            ChannelFileRead::Text(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(_) => ChannelFileRead::Named,
        Err(error) => {
            warn!(platform = %T::PLATFORM, %error, "channel file fetch failed");
            ChannelFileRead::Named
        }
    }
}

/// Bytes at a public http(s) URL, at most `max` of them. Any other handle is
/// `Ok(None)`: not this fetcher's to read.
pub(crate) async fn fetch_public(url: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Ok(None);
    }
    let (target, client) =
        crate::services::outbound_security::build_public_http_client(url, FETCH_TIMEOUT, None)
            .await?;
    let response = client
        .get(target)
        .send()
        .await
        .map_err(|_| "download failed".to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }
    crate::services::outbound_security::read_limited_body(response, max)
        .await
        .map(Some)
}

/// A public voice URL as audio of the format it was decoded with.
pub(crate) async fn fetch_public_voice(
    voice: &ChannelVoiceRef,
) -> Result<Option<VoiceAudio>, String> {
    Ok(fetch_public(&voice.source, CHANNEL_VOICE_BYTES)
        .await?
        .map(|bytes| VoiceAudio {
            bytes,
            format: voice.format.clone(),
        }))
}

/// A public file URL, capped at the size a text file is read to.
pub(crate) async fn fetch_public_file(file: &ChannelFileRef) -> Result<Option<Vec<u8>>, String> {
    fetch_public(&file.url, CHANNEL_TEXT_FILE_BYTES as usize).await
}
