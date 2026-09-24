//! Contract for `ai.webSearch` / `ai.groundingSearch`.
//!
//! Producers in `web_search` write [`capability_payload`]. Reply copy, display
//! hints, and analyze-step source lists recognize that object here. The
//! capability registry reuses the same input/output schemas so the two aliases
//! cannot drift.

use serde_json::{Value, json};

pub fn normalize_search_type(search_type: &str) -> &'static str {
    match search_type {
        "rss_source" => "rss_source",
        "api_docs" => "api_docs",
        _ => "general",
    }
}

pub fn capability_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "Search query" },
            "searchType": {
                "type": "string",
                "enum": ["rss_source", "api_docs", "general"],
                "default": "general",
                "description": "Search type: rss_source for RSS feeds, api_docs for API docs, general for a web search"
            },
            "maxResults": {
                "type": "integer",
                "default": 5,
                "description": "Maximum number of results"
            },
            "searchPrompt": {
                "type": "string",
                "description": "Custom search prompt"
            }
        },
        "required": ["query"]
    })
}

pub fn capability_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "success": { "type": "boolean" },
            "query": { "type": "string", "description": "Echoed query" },
            "searchType": { "type": "string", "description": "Echoed search type" },
            "aiSummary": { "type": "string", "description": "AI summary of the search results" },
            "results": { "type": "array", "description": "Search result list" },
            "totalResults": { "type": "integer", "description": "Number of results" }
        }
    })
}

pub fn capability_payload(
    query: &str,
    search_type: &str,
    ai_summary: String,
    results: Vec<Value>,
) -> Value {
    json!({
        "success": true,
        "query": query,
        "searchType": search_type,
        "aiSummary": ai_summary,
        "results": results,
        "totalResults": results.len()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_keys_match_declared_output_schema() {
        let out = capability_payload("q", "general", "s".into(), vec![json!({"name": "a"})]);
        let schema = capability_output_schema();
        let declared: Vec<String> = schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|props| props.keys().cloned().collect())
            .expect("schema properties");
        let produced = out.as_object().expect("payload object");
        for key in &declared {
            assert!(produced.contains_key(key), "payload missing declared {key}");
        }
        for key in produced.keys() {
            assert!(
                declared.iter().any(|declared| declared == key),
                "payload has undeclared {key}"
            );
        }
    }

    #[test]
    fn normalize_search_type_clamps_to_declared_enum() {
        assert_eq!(normalize_search_type("rss_source"), "rss_source");
        assert_eq!(normalize_search_type("api_docs"), "api_docs");
        assert_eq!(normalize_search_type("general"), "general");
        assert_eq!(normalize_search_type("nope"), "general");
        assert_eq!(normalize_search_type(""), "general");
    }

    #[test]
    fn input_schema_drops_unused_result_format_and_source() {
        let props = capability_input_schema()
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .expect("schema properties");
        assert!(props.contains_key("query"));
        assert!(props.contains_key("searchType"));
        assert!(props.contains_key("maxResults"));
        assert!(props.contains_key("searchPrompt"));
        assert!(!props.contains_key("resultFormat"));
        assert!(!props.contains_key("source"));
    }

    #[test]
    fn capability_payload_sets_search_shape() {
        let out = capability_payload("q", "general", "s".into(), vec![json!({"name": "a"})]);
        assert_eq!(out["success"], true);
        assert_eq!(out["searchType"], "general");
        assert_eq!(out["totalResults"], 1);
    }
}
