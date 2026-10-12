//! Whether the judgment model answers typed decisions or chat, as its source
//! says: OpenRouter lists a decisions model's output as `decisions`. Asked
//! once per model; a judgment never has to guess a model by its name.

use std::time::{Duration, Instant};

use crate::GLOBAL_DYNAMIC_CONFIG;

/// How long asking the source may hold up a judgment.
const ASK_TIMEOUT: Duration = Duration::from_secs(2);
/// A source that could not be reached is asked again after this long.
const RETRY_AFTER: Duration = Duration::from_secs(300);

/// Asks the judgment model's source what kind it is, unless already known.
pub async fn learn_judge_model_kind() {
    let Some(source) = GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_judge_model_source()
    else {
        return;
    };
    let model = source.model.trim().to_owned();
    // An alias (`~typesafe/jev-latest`) has no listing of its own.
    if model.starts_with('~') || crate::config::model_kind(&model).is_some() {
        return;
    }
    static TRIED: std::sync::Mutex<Option<(String, Instant)>> = std::sync::Mutex::new(None);
    if let Ok(mut tried) = TRIED.lock() {
        if tried
            .as_ref()
            .is_some_and(|(last, at)| *last == model && at.elapsed() < RETRY_AFTER)
        {
            return;
        }
        *tried = Some((model.clone(), Instant::now()));
    }
    match ask(&source.base_url, &model).await {
        Some(decisions) => {
            tracing::info!(model, decisions, "[ModelKind] Judgment model kind learned");
            crate::config::remember_model_kind(&model, decisions);
        }
        None => tracing::debug!(model, "[ModelKind] Source did not say; asking again later"),
    }
}

/// `Some(true)` when the source lists the model's output as decisions,
/// `Some(false)` when it lists it otherwise or has no such listing.
async fn ask(base_url: &str, model: &str) -> Option<bool> {
    let origin = url::Url::parse(base_url)
        .ok()?
        .origin()
        .ascii_serialization();
    let response = reqwest::Client::builder()
        .timeout(ASK_TIMEOUT)
        .build()
        .ok()?
        .get(format!("{origin}/api/v1/models/{model}/endpoints"))
        .send()
        .await
        .ok()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Some(false);
    }
    let listing: serde_json::Value = response.error_for_status().ok()?.json().await.ok()?;
    Some(answers_decisions(&listing))
}

fn answers_decisions(listing: &serde_json::Value) -> bool {
    listing["data"]["architecture"]["output_modalities"]
        .as_array()
        .is_some_and(|outputs| outputs.iter().any(|output| output == "decisions"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_model_listed_as_answering_decisions_is_one() {
        let decisions = json!({"data": {"id": "microsoft/microsoft-decision-1", "architecture":
            {"modality": "text->decisions", "input_modalities": ["text"], "output_modalities": ["decisions"]}}});
        let chat = json!({"data": {"id": "openai/gpt-6-luna", "architecture":
            {"modality": "text->text", "output_modalities": ["text"]}}});
        assert!(answers_decisions(&decisions));
        assert!(!answers_decisions(&chat));
        assert!(!answers_decisions(&json!({"error": {"code": 404}})));
    }
}
