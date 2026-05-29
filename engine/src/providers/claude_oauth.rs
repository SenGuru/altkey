//! Claude via Claude Code OAuth → Anthropic Messages API.
//! Port of Python's claude_oauth.py. Reads ~/.claude/.credentials.json, refreshes
//! via console.anthropic.com, talks api.anthropic.com/v1/messages with the
//! `oauth-2025-04-20` beta + Claude Code attestation system block.
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;
use crate::sse;
use crate::store;
use crate::translate;

const API: &str = "https://api.anthropic.com/v1/messages";
const MODELS_API: &str = "https://api.anthropic.com/v1/models";
const TOKEN_API: &str = "https://console.anthropic.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ATTEST: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const BETA: &str = "oauth-2025-04-20";

const OPUS: &str = "claude-opus-4-5-20251101";
const SONNET: &str = "claude-sonnet-4-5-20250929";
const HAIKU: &str = "claude-haiku-4-5-20251001";

pub const MODELS: &[&str] = &[
    "claude-opus-4-8",
    "claude-opus-4-5",
    "claude-sonnet-4-6",
    "claude-haiku-4-5",
    "claude-3-7-sonnet",
    "claude-3-5-sonnet",
    "claude-3-5-haiku",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Creds {
    access_token: String,
    refresh_token: String,
    expires_at: i64, // ms since epoch (matches Python's expiresAt)
    subscription_type: String,
}

static CACHED: Lazy<Mutex<Option<Creds>>> = Lazy::new(|| Mutex::new(None));

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read_live() -> Option<Creds> {
    let p = config::claude_creds_path();
    let raw = std::fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let o = v.get("claudeAiOauth")?;
    Some(Creds {
        access_token: o.get("accessToken")?.as_str()?.to_string(),
        refresh_token: o
            .get("refreshToken")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        expires_at: o
            .get("expiresAt")
            .and_then(|x| x.as_i64())
            .unwrap_or(0),
        subscription_type: o
            .get("subscriptionType")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
    })
}

pub fn is_connected() -> bool {
    CACHED.lock().is_some() || read_live().is_some()
}

async fn token() -> Result<String> {
    // Prefer live file if its access token is still valid.
    let live = read_live();
    if let Some(ref l) = live {
        if l.expires_at / 1000 > now_secs() + 60 && !l.access_token.is_empty() {
            *CACHED.lock() = Some(l.clone());
            return Ok(l.access_token.clone());
        }
    }
    // Fall back to in-memory + refresh.
    let mut current = CACHED.lock().clone();
    if current.is_none() {
        current = live.clone();
    }
    let mut current = current.ok_or_else(|| anyhow!("claude (oauth) not connected — log into Claude Code"))?;
    if current.expires_at / 1000 <= now_secs() + 60 {
        // Prefer live's newer refresh_token if any.
        if let Some(ref l) = live {
            if !l.refresh_token.is_empty() {
                current.refresh_token = l.refresh_token.clone();
            }
        }
        current = refresh(&current).await?;
    }
    *CACHED.lock() = Some(current.clone());
    Ok(current.access_token)
}

async fn refresh(creds: &Creds) -> Result<Creds> {
    if creds.refresh_token.is_empty() {
        return Err(anyhow!("claude oauth token expired and no refresh token"));
    }
    let client = reqwest::Client::new();
    let r = client
        .post(TOKEN_API)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": creds.refresh_token,
            "client_id": CLIENT_ID,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("refresh request failed")?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet = if text.len() > 200 { &text[..200] } else { &text };
        return Err(anyhow!("claude oauth refresh {}: {}", status, snippet));
    }
    let d: Value = serde_json::from_str(&text).context("refresh json")?;
    let access_token = d
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("refresh: no access_token"))?
        .to_string();
    let refresh_token = d
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .unwrap_or(&creds.refresh_token)
        .to_string();
    let expires_in = d.get("expires_in").and_then(|x| x.as_i64()).unwrap_or(3600);
    Ok(Creds {
        access_token,
        refresh_token,
        expires_at: (now_secs() + expires_in) * 1000,
        subscription_type: creds.subscription_type.clone(),
    })
}

/// Background-loop helper — only refreshes if a cached cred is close to expiring.
pub async fn refresh_if_due() -> Result<()> {
    let current = CACHED.lock().clone();
    let Some(c) = current else { return Ok(()); };
    if c.expires_at / 1000 <= now_secs() + 600 {
        let new = refresh(&c).await?;
        *CACHED.lock() = Some(new);
    }
    Ok(())
}

fn resolve_model(model: &str) -> String {
    let detected = store::load_detected_models("claude_oauth")
        .map(|d| d.models)
        .unwrap_or_default();
    let m = model.to_lowercase();
    let fam = if m.contains("opus") {
        "opus"
    } else if m.contains("haiku") {
        "haiku"
    } else {
        "sonnet"
    };
    if detected.iter().any(|x| x == model) {
        return model.to_string();
    }
    if let Some(found) = detected.iter().find(|x| x.contains(fam)) {
        return found.clone();
    }
    match fam {
        "opus" => OPUS,
        "haiku" => HAIKU,
        _ => SONNET,
    }
    .to_string()
}

fn headers(token: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut h = HeaderMap::new();
    h.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
    );
    h.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    h.insert("anthropic-beta", HeaderValue::from_static(BETA));
    h.insert("content-type", HeaderValue::from_static("application/json"));
    h.insert(
        "user-agent",
        HeaderValue::from_static("claude-cli/1.0.0 (external)"),
    );
    h.insert("x-app", HeaderValue::from_static("cli"));
    h
}

// ---- OpenAI <-> Anthropic translation -------------------------------------

fn content_to_anthropic(content: &Value) -> Vec<Value> {
    match content {
        Value::String(s) => vec![json!({"type": "text", "text": s})],
        Value::Array(arr) => {
            let mut blocks = Vec::new();
            for part in arr {
                let Some(t) = part.get("type").and_then(|x| x.as_str()) else {
                    continue;
                };
                if t == "text" {
                    blocks.push(json!({
                        "type": "text",
                        "text": part.get("text").and_then(|x| x.as_str()).unwrap_or(""),
                    }));
                } else if t == "image_url" {
                    let url = part
                        .get("image_url")
                        .and_then(|x| x.get("url"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    if let Some(rest) = url.strip_prefix("data:") {
                        if let Some(comma) = rest.find(',') {
                            let header = &rest[..comma];
                            let b64 = &rest[comma + 1..];
                            let media = header.split(';').next().unwrap_or("image/png");
                            blocks.push(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media,
                                    "data": b64,
                                }
                            }));
                        }
                    }
                }
            }
            if blocks.is_empty() {
                vec![json!({"type": "text", "text": ""})]
            } else {
                blocks
            }
        }
        _ => vec![json!({"type": "text", "text": ""})],
    }
}

fn to_anthropic(req: &Value) -> Value {
    let mut system_blocks = vec![json!({"type": "text", "text": ATTEST})];
    let mut messages = Vec::new();

    if let Some(arr) = req.get("messages").and_then(|x| x.as_array()) {
        for m in arr {
            let role = m.get("role").and_then(|x| x.as_str()).unwrap_or("user");
            if role == "system" {
                let txt = match m.get("content") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Array(arr)) => arr
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|x| x.as_str()).map(String::from))
                        .collect::<Vec<_>>()
                        .join(""),
                    _ => String::new(),
                };
                system_blocks.push(json!({"type": "text", "text": txt}));
            } else if role == "tool" {
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.get("tool_call_id").and_then(|x| x.as_str()).unwrap_or(""),
                        "content": m.get("content").cloned().unwrap_or(Value::String(String::new())),
                    }]
                }));
            } else if role == "assistant" && m.get("tool_calls").is_some() {
                let mut blocks = Vec::new();
                if let Some(Value::String(c)) = m.get("content") {
                    if !c.is_empty() {
                        blocks.push(json!({"type": "text", "text": c}));
                    }
                }
                if let Some(tcs) = m.get("tool_calls").and_then(|x| x.as_array()) {
                    for tc in tcs {
                        let fn_v = tc.get("function").cloned().unwrap_or(json!({}));
                        let name = fn_v.get("name").and_then(|x| x.as_str()).unwrap_or("");
                        let args_str = fn_v
                            .get("arguments")
                            .and_then(|x| x.as_str())
                            .unwrap_or("{}");
                        let args: Value =
                            serde_json::from_str(args_str).unwrap_or(Value::Object(Map::new()));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": tc.get("id").and_then(|x| x.as_str()).unwrap_or(""),
                            "name": name,
                            "input": args,
                        }));
                    }
                }
                messages.push(json!({"role": "assistant", "content": blocks}));
            } else {
                let content = content_to_anthropic(m.get("content").unwrap_or(&Value::Null));
                messages.push(json!({"role": role, "content": content}));
            }
        }
    }

    let model = req
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("claude-sonnet-4-6");
    let mut body = json!({
        "model": resolve_model(model),
        "max_tokens": req.get("max_tokens").and_then(|x| x.as_i64()).unwrap_or(8192),
        "system": system_blocks,
        "messages": messages,
    });
    if let Some(t) = req.get("temperature") {
        body["temperature"] = t.clone();
    }
    if let Some(tools) = req.get("tools").and_then(|x| x.as_array()) {
        let translated: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|x| x.as_str()) == Some("function"))
            .filter_map(|t| {
                let f = t.get("function")?;
                Some(json!({
                    "name": f.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                    "description": f.get("description").and_then(|x| x.as_str()).unwrap_or(""),
                    "input_schema": f.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}})),
                }))
            })
            .collect();
        if !translated.is_empty() {
            body["tools"] = Value::Array(translated);
        }
    }
    match req.get("tool_choice") {
        Some(Value::String(s)) if s == "required" => {
            body["tool_choice"] = json!({"type": "any"});
        }
        Some(Value::Object(o)) => {
            if let Some(name) = o
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
            {
                body["tool_choice"] = json!({"type": "tool", "name": name});
            }
        }
        _ => {}
    }
    body
}

// ---- non-streaming completion ---------------------------------------------

pub async fn openai_completion(req: &Value) -> Result<Value> {
    let token = token().await?;
    let body = to_anthropic(req);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let r = client.post(API).headers(headers(&token)).json(&body).send().await?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet = if text.len() > 400 { &text[..400] } else { &text };
        return Err(anyhow!("claude oauth {}: {}", status, snippet));
    }
    let d: Value = serde_json::from_str(&text)?;
    let mut content_text = String::new();
    let mut tool_calls = Vec::new();
    if let Some(arr) = d.get("content").and_then(|x| x.as_array()) {
        for b in arr {
            match b.get("type").and_then(|x| x.as_str()) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                        content_text.push_str(t);
                    }
                }
                Some("tool_use") => {
                    let id = b.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let input = b.get("input").cloned().unwrap_or(json!({}));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".into()),
                        }
                    }));
                }
                _ => {}
            }
        }
    }
    let mut msg = json!({
        "role": "assistant",
        "content": if content_text.is_empty() { Value::Null } else { Value::String(content_text) },
    });
    if !tool_calls.is_empty() {
        msg["tool_calls"] = Value::Array(tool_calls.clone());
    }
    let finish = if tool_calls.is_empty() { "stop" } else { "tool_calls" };
    let usage = d.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
    Ok(json!({
        "id": translate::chatcmpl_id(),
        "object": "chat.completion",
        "created": translate::unix_now(),
        "model": req.get("model").cloned().unwrap_or(Value::Null),
        "choices": [{ "index": 0, "message": msg, "finish_reason": finish }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens,
        }
    }))
}

// ---- streaming completion --------------------------------------------------

pub fn openai_stream(req: Value) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
    let req = req.clone();
    let s = async_stream::stream! {
        let token = match token().await {
            Ok(t) => t,
            Err(e) => {
                yield Ok(Bytes::from(sse::data_line(&serde_json::to_string(&json!({
                    "error": {"message": format!("claude oauth: {}", e), "type": "upstream_error"}
                })).unwrap())));
                yield Ok(Bytes::from(sse::done_line()));
                return;
            }
        };
        let mut body = to_anthropic(&req);
        body["stream"] = Value::Bool(true);
        let cid = translate::chatcmpl_id();
        let created = translate::unix_now();
        let model = req.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mk_chunk = |delta: Value, finish: Option<&str>| -> String {
            let chunk = json!({
                "id": cid,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
            });
            sse::data_line(&chunk.to_string())
        };
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build() {
            Ok(c) => c,
            Err(e) => {
                yield Ok(Bytes::from(mk_chunk(json!({"error": e.to_string()}), Some("stop"))));
                yield Ok(Bytes::from(sse::done_line()));
                return;
            }
        };
        let resp = client.post(API).headers(headers(&token)).json(&body).send().await;
        let r = match resp {
            Ok(r) => r,
            Err(e) => {
                yield Ok(Bytes::from(mk_chunk(json!({"error": e.to_string()}), Some("stop"))));
                yield Ok(Bytes::from(sse::done_line()));
                return;
            }
        };
        if !r.status().is_success() {
            let status = r.status();
            let err_body = r.text().await.unwrap_or_default();
            let snippet: String = err_body.chars().take(300).collect();
            yield Ok(Bytes::from(sse::data_line(&serde_json::to_string(&json!({
                "error": {"message": format!("claude oauth {}: {}", status, snippet), "type": "upstream_error"}
            })).unwrap())));
            yield Ok(Bytes::from(sse::done_line()));
            return;
        }
        yield Ok(Bytes::from(mk_chunk(json!({"role": "assistant", "content": ""}), None)));

        let mut tool_idx: i64 = -1;
        let mut finish = "stop";
        let mut byte_stream = r.bytes_stream();
        let mut leftover = String::new();
        while let Some(chunk) = byte_stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => break,
            };
            leftover.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(nl_pos) = leftover.find('\n') {
                let line: String = leftover.drain(..=nl_pos).collect();
                let line = line.trim_end_matches(['\r', '\n']).trim().to_string();
                if !line.starts_with("data:") {
                    continue;
                }
                let data = line[5..].trim().to_string();
                if data.is_empty() {
                    continue;
                }
                let Ok(evt): Result<Value, _> = serde_json::from_str(&data) else { continue; };
                let et = evt.get("type").and_then(|x| x.as_str()).unwrap_or("");
                match et {
                    "content_block_start" => {
                        if let Some(blk) = evt.get("content_block") {
                            if blk.get("type").and_then(|x| x.as_str()) == Some("tool_use") {
                                tool_idx += 1;
                                let id = blk.get("id").and_then(|x| x.as_str()).unwrap_or("");
                                let name = blk.get("name").and_then(|x| x.as_str()).unwrap_or("");
                                finish = "tool_calls";
                                yield Ok(Bytes::from(mk_chunk(json!({
                                    "tool_calls": [{"index": tool_idx, "id": id, "type": "function",
                                                    "function": {"name": name, "arguments": ""}}]
                                }), None)));
                            }
                        }
                    }
                    "content_block_delta" => {
                        if let Some(d) = evt.get("delta") {
                            match d.get("type").and_then(|x| x.as_str()) {
                                Some("text_delta") => {
                                    let t = d.get("text").and_then(|x| x.as_str()).unwrap_or("");
                                    yield Ok(Bytes::from(mk_chunk(json!({"content": t}), None)));
                                }
                                Some("input_json_delta") => {
                                    let pj = d.get("partial_json").and_then(|x| x.as_str()).unwrap_or("");
                                    yield Ok(Bytes::from(mk_chunk(json!({
                                        "tool_calls": [{"index": tool_idx, "function": {"arguments": pj}}]
                                    }), None)));
                                }
                                _ => {}
                            }
                        }
                    }
                    "message_delta" => {
                        if evt.get("delta")
                            .and_then(|x| x.get("stop_reason"))
                            .and_then(|x| x.as_str()) == Some("tool_use") {
                            finish = "tool_calls";
                        }
                    }
                    "message_stop" => break,
                    _ => {}
                }
            }
        }
        yield Ok(Bytes::from(mk_chunk(json!({}), Some(finish))));
        yield Ok(Bytes::from(sse::done_line()));
    };
    Box::pin(s)
}

// ---- native /v1/messages passthrough -------------------------------------

fn inject_attestation(mut body: Value) -> Value {
    let attest = json!({"type": "text", "text": ATTEST});
    let new_system = match body.get("system").cloned() {
        None => Value::Array(vec![attest]),
        Some(Value::String(s)) => Value::Array(vec![attest, json!({"type": "text", "text": s})]),
        Some(Value::Array(mut arr)) => {
            let already = arr
                .first()
                .and_then(|x| x.get("text"))
                .and_then(|x| x.as_str())
                .map(|s| s.starts_with("You are Claude Code"))
                .unwrap_or(false);
            if already {
                Value::Array(arr)
            } else {
                let mut out = vec![attest];
                out.append(&mut arr);
                Value::Array(out)
            }
        }
        Some(other) => Value::Array(vec![attest, other]),
    };
    body["system"] = new_system;
    let model = body
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("claude-sonnet-4-6");
    body["model"] = Value::String(resolve_model(model));
    body
}

pub async fn anthropic_messages(body: Value) -> Result<(reqwest::StatusCode, Value)> {
    let token = token().await?;
    let mut fwd = inject_attestation(body);
    if let Value::Object(ref mut m) = fwd {
        m.remove("stream");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let r = client.post(API).headers(headers(&token)).json(&fwd).send().await?;
    let status = r.status();
    let txt = r.text().await.unwrap_or_default();
    let val: Value =
        serde_json::from_str(&txt).unwrap_or_else(|_| json!({"error": txt.chars().take(400).collect::<String>()}));
    Ok((status, val))
}

pub fn anthropic_messages_stream(body: Value) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
    let s = async_stream::stream! {
        let token = match token().await {
            Ok(t) => t,
            Err(e) => {
                yield Ok(Bytes::from(format!("event: error\ndata: {{\"type\":\"error\",\"error\":{{\"message\":\"{}\"}}}}\n\n", e)));
                return;
            }
        };
        let mut fwd = inject_attestation(body);
        fwd["stream"] = Value::Bool(true);
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build() { Ok(c) => c, Err(_) => return };
        let resp = client.post(API).headers(headers(&token)).json(&fwd).send().await;
        let r = match resp {
            Ok(r) => r,
            Err(e) => {
                yield Ok(Bytes::from(format!("event: error\ndata: {{\"type\":\"error\",\"error\":{{\"message\":\"{}\"}}}}\n\n", e)));
                return;
            }
        };
        if !r.status().is_success() {
            let err = r.text().await.unwrap_or_default();
            let snippet: String = err.chars().take(300).collect();
            yield Ok(Bytes::from(format!("event: error\ndata: {{\"type\":\"error\",\"error\":{{\"message\":\"{}\"}}}}\n\n", snippet)));
            return;
        }
        let mut bs = r.bytes_stream();
        while let Some(c) = bs.next().await {
            if let Ok(c) = c {
                yield Ok(c);
            }
        }
    };
    Box::pin(s)
}

// ---- detect ----------------------------------------------------------------

pub async fn detect() -> Result<Vec<String>> {
    let token = token().await?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()?;
    let r = client.get(MODELS_API).headers(headers(&token)).send().await?;
    if !r.status().is_success() {
        return Ok(Vec::new());
    }
    let d: Value = r.json().await?;
    let mut ids: Vec<String> = d
        .get("data")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    ids.sort_by(|a, b| b.cmp(a));
    if !ids.is_empty() {
        let _ = store::save_detected_models("claude_oauth", &ids);
    }
    Ok(ids)
}

// ---- OAuth flow (browser-based) --------------------------------------------

const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const SCOPES: &str = "user:inference";

fn b64url(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn redirect_uri() -> String {
    let port = std::env::var("ALTKEY_PORT").unwrap_or_else(|_| "8787".into());
    format!("http://localhost:{}/callback", port)
}

#[derive(Default)]
struct PkceState {
    verifier: String,
    state: String,
}

static PKCE: Lazy<Mutex<Option<PkceState>>> = Lazy::new(|| Mutex::new(None));

pub fn start_oauth() -> Value {
    use rand::RngCore;
    use sha2::Digest;
    let mut v = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut v);
    let verifier = b64url(&v);
    let challenge = b64url(&sha2::Sha256::digest(verifier.as_bytes()));
    let mut s = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut s);
    let state = b64url(&s);
    *PKCE.lock() = Some(PkceState {
        verifier: verifier.clone(),
        state: state.clone(),
    });
    let url = format!(
        "{authorize}?code=true&client_id={cid}&response_type=code&redirect_uri={ru}&scope={scope}&code_challenge={ch}&code_challenge_method=S256&state={st}",
        authorize = AUTHORIZE_URL,
        cid = CLIENT_ID,
        ru = url::form_urlencoded::byte_serialize(redirect_uri().as_bytes()).collect::<String>(),
        scope = url::form_urlencoded::byte_serialize(SCOPES.as_bytes()).collect::<String>(),
        ch = challenge,
        st = state,
    );
    json!({ "url": url })
}

pub async fn finish_oauth(code_input: &str) -> Result<Value> {
    let pkce = PKCE
        .lock()
        .take()
        .ok_or_else(|| anyhow!("no pending OAuth flow"))?;
    let raw = code_input.trim().to_string();
    let (code, state) = match raw.split_once('#') {
        Some((c, s)) => (c.to_string(), s.to_string()),
        None => (raw, pkce.state.clone()),
    };
    let client = reqwest::Client::new();
    let r = client
        .post(TOKEN_API)
        .json(&json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect_uri(),
            "client_id": CLIENT_ID,
            "code_verifier": pkce.verifier,
            "state": state,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet = if text.len() > 300 { &text[..300] } else { &text };
        return Err(anyhow!("oauth exchange {}: {}", status, snippet));
    }
    let d: Value = serde_json::from_str(&text)?;
    let access_token = d
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("no access_token"))?
        .to_string();
    let refresh_token = d
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let expires_in = d.get("expires_in").and_then(|x| x.as_i64()).unwrap_or(28800);
    let sub = d
        .get("account")
        .and_then(|a| a.get("subscription_type"))
        .and_then(|x| x.as_str())
        .unwrap_or("unknown")
        .to_string();
    *CACHED.lock() = Some(Creds {
        access_token,
        refresh_token,
        expires_at: (now_secs() + expires_in) * 1000,
        subscription_type: sub.clone(),
    });
    Ok(json!({"ok": true, "subscription": sub}))
}

pub fn cache_from_file() {
    if let Some(c) = read_live() {
        *CACHED.lock() = Some(c);
    }
}
