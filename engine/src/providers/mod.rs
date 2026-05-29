//! Provider routing + live session detection.
//!
//! Phase 1 wires two providers (Claude OAuth, ChatGPT Codex OAuth). Gemini is
//! parked per Phase 0 — there's no free OAuth image-gen path on Google, and
//! ChatGPT + Claude already cover the full capability matrix between them.
pub mod chatgpt;
pub mod claude_oauth;

use serde_json::{json, Value};

use crate::store;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Chatgpt,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Chatgpt => "chatgpt",
        }
    }
}

pub fn for_model(model: &str) -> Option<Provider> {
    let m = model.to_lowercase();
    if m.starts_with("claude") {
        return Some(Provider::Claude);
    }
    for prefix in ["gpt", "o1", "o3", "o4", "chatgpt"] {
        if m.starts_with(prefix) {
            return Some(Provider::Chatgpt);
        }
    }
    None
}

pub fn list_models() -> Vec<Value> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // (sess-name-for-cache, provider, static-fallback-models)
    let plans: [(&str, &str, &[&str]); 2] = [
        ("claude_oauth", "claude", claude_oauth::MODELS),
        ("chatgpt", "chatgpt", chatgpt::MODELS),
    ];
    for (cache_name, prov_name, static_models) in plans {
        let detected = store::load_detected_models(cache_name)
            .map(|d| d.models)
            .unwrap_or_default();
        let combined: Vec<String> = detected
            .iter()
            .cloned()
            .chain(static_models.iter().map(|s| s.to_string()))
            .collect();
        for mid in combined {
            if mid.is_empty() {
                continue;
            }
            let key = format!("{}::{}", prov_name, mid);
            if seen.insert(key) {
                out.push(json!({
                    "id": mid,
                    "object": "model",
                    "owned_by": prov_name,
                }));
            }
        }
    }
    out
}

pub async fn detect_all() -> Value {
    let mut results = serde_json::Map::new();
    // claude
    if claude_oauth::is_connected() {
        match claude_oauth::detect().await {
            Ok(ids) => {
                results.insert("claude".into(), json!(ids));
            }
            Err(e) => {
                results.insert("claude".into(), json!({"error": short_err(&e)}));
            }
        }
    }
    // chatgpt
    if chatgpt::is_connected() {
        match chatgpt::detect().await {
            Ok(ids) => {
                results.insert("chatgpt".into(), json!(ids));
            }
            Err(e) => {
                results.insert("chatgpt".into(), json!({"error": short_err(&e)}));
            }
        }
    }
    Value::Object(results)
}

pub async fn detect_one(provider: &str) -> Vec<String> {
    match provider {
        "claude" | "claude_oauth" => claude_oauth::detect().await.unwrap_or_default(),
        "chatgpt" => chatgpt::detect().await.unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// What /admin/status reports — sessions derived from CLI-cred-file presence
/// plus Phase 0 Python-DB rows if the user's still got those around.
pub fn live_sessions() -> Vec<Value> {
    let mut out = Vec::new();
    if claude_oauth::is_connected() {
        out.push(json!({"provider": "claude_oauth", "updated_at": store::now_unix()}));
    }
    if chatgpt::is_connected() {
        out.push(json!({"provider": "chatgpt", "updated_at": store::now_unix()}));
    }
    out
}

fn short_err(e: &anyhow::Error) -> String {
    let s = format!("{}", e);
    if s.len() > 200 { s[..200].to_string() } else { s }
}
