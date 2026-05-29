//! Shared translation helpers between OpenAI shape and provider shapes.
//! Provider-specific logic lives in each providers/<name>.rs; this file holds
//! only generic utilities used by more than one provider.
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn chatcmpl_id() -> String {
    format!("chatcmpl-{}", Uuid::new_v4().simple().to_string()[..24].to_string())
}

pub fn resp_id() -> String {
    format!("resp_{}", Uuid::new_v4().simple().to_string()[..24].to_string())
}

/// Extracts plain text from an OpenAI `content` field which can be a string
/// or an array of content parts. Non-text parts (images, etc) are skipped.
pub fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|p| {
                let t = p.get("type")?.as_str()?;
                if t == "text" {
                    Some(p.get("text")?.as_str()?.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// One streaming OpenAI chat.completion.chunk with optional content delta and
/// finish reason. Matches Python's openai_chunk shape exactly.
pub fn openai_chunk(model: &str, delta_text: &str, finish: Option<&str>) -> Value {
    let delta = if delta_text.is_empty() {
        json!({})
    } else {
        json!({ "content": delta_text })
    };
    json!({
        "id": chatcmpl_id(),
        "object": "chat.completion.chunk",
        "created": unix_now(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish,
        }],
    })
}

/// One streaming OpenAI tool-call chunk (used by ChatGPT path).
pub fn openai_tool_call_chunk(model: &str, index: usize, id: &str, name: &str, args_delta: &str) -> Value {
    let mut function = serde_json::Map::new();
    if !name.is_empty() {
        function.insert("name".into(), Value::String(name.into()));
    }
    function.insert("arguments".into(), Value::String(args_delta.into()));
    json!({
        "id": chatcmpl_id(),
        "object": "chat.completion.chunk",
        "created": unix_now(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": { "tool_calls": [{
                "index": index,
                "id": id,
                "type": "function",
                "function": Value::Object(function),
            }]},
            "finish_reason": null,
        }],
    })
}
