//! Voices and files of a private message, read into text before Work starts.
//! Fetching a voice is each platform's own; transcribing it and reading a
//! file are shared.

use std::time::Duration;

use myriad_agent_rules::channel::{
    CHANNEL_TEXT_FILE_BYTES, ChannelFileRead, ChannelFileRef, channel_file_is_text,
};
use tracing::warn;

const FILE_TIMEOUT: Duration = Duration::from_secs(20);

/// What a voice said, through the site's speech service. `None` when nothing
/// usable came back.
pub(crate) async fn transcribe_voice(audio: Vec<u8>, format: &str) -> Option<String> {
    if audio.is_empty() {
        return None;
    }
    match crate::services::speech_runtime::transcribe_bytes(audio, format, Some("zh")).await {
        Ok(text) => Some(text.trim().to_string()).filter(|text| !text.is_empty()),
        Err(error) => {
            warn!(%error, "channel voice transcription failed");
            None
        }
    }
}

/// Each file with its text when it is a small text file that downloads from
/// a public address; otherwise only named.
pub(crate) async fn read_files(files: &[ChannelFileRef]) -> Vec<(ChannelFileRef, ChannelFileRead)> {
    let mut read = Vec::with_capacity(files.len());
    for file in files {
        let text = if file.url.is_empty() || !channel_file_is_text(&file.name, file.size) {
            None
        } else {
            match download_text(&file.url).await {
                Ok(text) => Some(text),
                Err(error) => {
                    warn!(%error, "channel file download failed");
                    None
                }
            }
        };
        let outcome = text.map_or(ChannelFileRead::Named, ChannelFileRead::Text);
        read.push((file.clone(), outcome));
    }
    read
}

async fn download_text(url: &str) -> Result<String, String> {
    let (target, client) =
        crate::services::outbound_security::build_public_http_client(url, FILE_TIMEOUT, None)
            .await?;
    let response = client
        .get(target)
        .send()
        .await
        .map_err(|_| "file download failed".to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }
    let bytes = crate::services::outbound_security::read_limited_body(
        response,
        CHANNEL_TEXT_FILE_BYTES as usize,
    )
    .await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
