//! ChatGPT via Codex OAuth → chatgpt.com/backend-api/codex/responses (OpenAI
//! Responses API). Port of Python's chatgpt.py. Reads ~/.codex/auth.json,
//! refreshes via auth.openai.com. Supports chat, streaming, tool calls, vision,
//! image generation (via image_generation tool), and a raw /v1/responses proxy.
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;
use crate::sse;
use crate::store;
use crate::translate;

const RESPONSES: &str = "https://chatgpt.com/backend-api/codex/responses";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ORIGINATOR: &str = "codex_cli_rs";
const USER_AGENT: &str = "codex_cli_rs/0.135.0";
const OPENAI_BETA: &str = "responses=experimental";

pub const MODELS: &[&str] = &["gpt-5.5", "gpt-5.4", "gpt-5.2"];
const DEFAULT: &str = "gpt-5.5";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Creds {
    access_token: String,
    refresh_token: String,
    account_id: String,
    /// ms since epoch (matches Python's expiresAt)
    expires_at: i64,
}

static CACHED: Lazy<Mutex<Option<Creds>>> = Lazy::new(|| Mutex::new(None));

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read_codex_file() -> Option<Creds> {
    let raw = std::fs::read_to_string(config::codex_creds_path()).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let t = v.get("tokens")?;
    let access_token = t
        .get("access_token")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    // Decode JWT exp claim so we only refresh when the access_token is actually
    // close to expiring. Codex auth.json rotates the file but doesn't store
    // an explicit expiry — exp lives inside the JWT payload.
    let expires_at_ms = jwt_exp_ms(&access_token).unwrap_or(0);
    Some(Creds {
        access_token,
        refresh_token: t.get("refresh_token")?.as_str()?.to_string(),
        account_id: t.get("account_id")?.as_str()?.to_string(),
        expires_at: expires_at_ms,
    })
}

fn jwt_exp_ms(jwt: &str) -> Option<i64> {
    use base64::Engine as _;
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    // Use URL_SAFE_NO_PAD; tolerate inputs that include padding by trimming.
    let payload_b64 = parts[1].trim_end_matches('=');
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64.as_bytes())
        .ok()?;
    let v: Value = serde_json::from_slice(&decoded).ok()?;
    let exp = v.get("exp").and_then(|x| x.as_i64())?;
    Some(exp.saturating_mul(1000))
}

pub fn is_connected() -> bool {
    CACHED.lock().is_some() || read_codex_file().is_some()
}

fn resolve_model(model: &str) -> String {
    if model.is_empty() {
        return DEFAULT.into();
    }
    if MODELS.iter().any(|m| *m == model) {
        return model.to_string();
    }
    // Aliases — anything unknown collapses to DEFAULT.
    let m = model.to_lowercase();
    let routed: &str = match m.as_str() {
        "gpt-4o" | "gpt-4.1" | "gpt-5" | "chatgpt-4o-latest" | "o3" => DEFAULT,
        "gpt-4o-mini" | "gpt-5-mini" | "o4-mini" => "gpt-5.4",
        "gpt-5.5" | "gpt-5.4" | "gpt-5.2" => return m,
        _ => DEFAULT,
    };
    routed.to_string()
}

fn headers(token: &str, account_id: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut h = HeaderMap::new();
    h.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
    );
    h.insert(
        "chatgpt-account-id",
        HeaderValue::from_str(account_id).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    h.insert("OpenAI-Beta", HeaderValue::from_static(OPENAI_BETA));
    h.insert("Content-Type", HeaderValue::from_static("application/json"));
    h.insert("originator", HeaderValue::from_static(ORIGINATOR));
    h.insert("User-Agent", HeaderValue::from_static(USER_AGENT));
    h
}

// ---- creds / refresh -------------------------------------------------------

fn current_creds() -> Result<Creds> {
    if let Some(c) = CACHED.lock().clone() {
        return Ok(c);
    }
    let seed = read_codex_file()
        .ok_or_else(|| anyhow!("chatgpt (oauth) not connected — log into the Codex CLI"))?;
    *CACHED.lock() = Some(seed.clone());
    Ok(seed)
}

async fn refresh(creds: &Creds) -> Result<Creds> {
    let client = reqwest::Client::new();
    let r = client
        .post(TOKEN_URL)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": creds.refresh_token,
            "client_id": CLIENT_ID,
            "scope": "openid profile email offline_access",
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("refresh request failed")?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet = if text.len() > 200 { &text[..200] } else { &text };
        return Err(anyhow!("chatgpt oauth refresh {}: {}", status, snippet));
    }
    let d: Value = serde_json::from_str(&text)?;
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
    let new = Creds {
        access_token: access_token.clone(),
        refresh_token: refresh_token.clone(),
        account_id: creds.account_id.clone(),
        expires_at: (now_secs() + expires_in) * 1000,
    };
    *CACHED.lock() = Some(new.clone());
    // Write the rotated tokens back to ~/.codex/auth.json so subsequent runs
    // (and the Codex CLI itself) see the latest refresh token. The Python
    // engine cached in its own sqlite, which left auth.json stale and led to
    // "refresh token already used" errors on engine swap.
    write_back_codex_file(&access_token, &refresh_token, &creds.account_id);
    Ok(new)
}

fn write_back_codex_file(access_token: &str, refresh_token: &str, account_id: &str) {
    let path = config::codex_creds_path();
    // Re-read the current file so we don't clobber unrelated keys.
    let mut v: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let tokens = v.get_mut("tokens").and_then(|t| t.as_object_mut());
    if let Some(tokens) = tokens {
        tokens.insert("access_token".into(), Value::String(access_token.into()));
        tokens.insert("refresh_token".into(), Value::String(refresh_token.into()));
        if tokens.get("account_id").is_none() {
            tokens.insert("account_id".into(), Value::String(account_id.into()));
        }
    } else if let Value::Object(ref mut root) = v {
        root.insert(
            "tokens".into(),
            json!({
                "access_token": access_token,
                "refresh_token": refresh_token,
                "account_id": account_id,
            }),
        );
    }
    if let Ok(serialized) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(&path, serialized);
    }
}

async fn token_and_account() -> Result<(String, String)> {
    let mut c = current_creds()?;
    if c.access_token.is_empty() || c.expires_at / 1000 <= now_secs() + 60 {
        c = refresh(&c).await?;
    }
    Ok((c.access_token, c.account_id))
}

// ---- OpenAI -> Codex Responses body ---------------------------------------

fn content_to_responses(content: &Value) -> Vec<Value> {
    match content {
        Value::String(s) => vec![json!({"type": "input_text", "text": s})],
        Value::Array(arr) => {
            let mut out = Vec::new();
            for part in arr {
                let Some(t) = part.get("type").and_then(|x| x.as_str()) else { continue; };
                if t == "text" {
                    out.push(json!({
                        "type": "input_text",
                        "text": part.get("text").and_then(|x| x.as_str()).unwrap_or(""),
                    }));
                } else if t == "image_url" {
                    let url = part
                        .get("image_url")
                        .and_then(|x| x.get("url"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    out.push(json!({"type": "input_image", "image_url": url}));
                }
            }
            if out.is_empty() {
                vec![json!({"type": "input_text", "text": ""})]
            } else {
                out
            }
        }
        _ => vec![json!({"type": "input_text", "text": ""})],
    }
}

/// Returns (instructions, input_items). System messages → instructions; tool
/// messages → function_call_output items; assistant tool_calls → function_call
/// items; everything else → "message" items.
fn build_input(messages: &Value) -> (String, Vec<Value>) {
    let mut instructions: Vec<String> = Vec::new();
    let mut items: Vec<Value> = Vec::new();
    let Some(arr) = messages.as_array() else {
        return ("You are a helpful assistant.".to_string(), items);
    };
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
            if !txt.is_empty() {
                instructions.push(txt);
            }
        } else if role == "tool" {
            items.push(json!({
                "type": "function_call_output",
                "call_id": m.get("tool_call_id").and_then(|x| x.as_str()).unwrap_or(""),
                "output": match m.get("content") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                },
            }));
        } else if role == "assistant" && m.get("tool_calls").is_some() {
            if let Some(tcs) = m.get("tool_calls").and_then(|x| x.as_array()) {
                for tc in tcs {
                    let fn_v = tc.get("function").cloned().unwrap_or(json!({}));
                    items.push(json!({
                        "type": "function_call",
                        "call_id": tc.get("id").and_then(|x| x.as_str()).unwrap_or(""),
                        "name": fn_v.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                        "arguments": fn_v.get("arguments").and_then(|x| x.as_str()).unwrap_or("{}"),
                    }));
                }
            }
            if let Some(Value::String(c)) = m.get("content") {
                if !c.is_empty() {
                    items.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": c}],
                    }));
                }
            }
        } else {
            items.push(json!({
                "type": "message",
                "role": role,
                "content": content_to_responses(m.get("content").unwrap_or(&Value::Null)),
            }));
        }
    }
    let instr = if instructions.is_empty() {
        "You are a helpful assistant.".to_string()
    } else {
        instructions.join("\n\n")
    };
    (instr, items)
}

fn translate_tools(tools: Option<&Value>) -> Option<Vec<Value>> {
    let arr = tools?.as_array()?;
    let out: Vec<Value> = arr
        .iter()
        .filter(|t| t.get("type").and_then(|x| x.as_str()) == Some("function"))
        .filter_map(|t| {
            let f = t.get("function")?;
            Some(json!({
                "type": "function",
                "name": f.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                "description": f.get("description").and_then(|x| x.as_str()).unwrap_or(""),
                "parameters": f.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}})),
            }))
        })
        .collect();
    if out.is_empty() { None } else { Some(out) }
}

fn build_body(req: &Value, stream: bool) -> Value {
    let (instructions, items) = build_input(req.get("messages").unwrap_or(&Value::Null));
    let model = resolve_model(req.get("model").and_then(|x| x.as_str()).unwrap_or(DEFAULT));
    let mut body = json!({
        "model": model,
        "instructions": instructions,
        "input": items,
        "stream": stream,
        "store": false,
    });
    if let Some(tools) = translate_tools(req.get("tools")) {
        body["tools"] = Value::Array(tools);
    }
    body
}

// ---- non-streaming completion (stream-and-aggregate) -----------------------

pub async fn openai_completion(req: &Value) -> Result<Value> {
    let (token, acct) = token_and_account().await?;
    let body = build_body(req, true);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let r = client
        .post(RESPONSES)
        .headers(headers(&token, &acct))
        .json(&body)
        .send()
        .await?;
    if !r.status().is_success() {
        let status = r.status();
        let txt = r.text().await.unwrap_or_default();
        let snip: String = txt.chars().take(400).collect();
        return Err(anyhow!("chatgpt {}: {}", status, snip));
    }
    let mut text = String::new();
    let mut tools_by_idx: BTreeMap<i64, (String, String, String)> = BTreeMap::new();
    let mut idx: i64 = -1;
    let mut usage = json!({});
    let mut bs = r.bytes_stream();
    let mut leftover = String::new();
    while let Some(c) = bs.next().await {
        let c = match c {
            Ok(c) => c,
            Err(_) => break,
        };
        leftover.push_str(&String::from_utf8_lossy(&c));
        while let Some(nl) = leftover.find('\n') {
            let line: String = leftover.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']).trim().to_string();
            if !line.starts_with("data:") {
                continue;
            }
            let Ok(ev): Result<Value, _> = serde_json::from_str(line[5..].trim()) else { continue; };
            let et = ev.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match et {
                "response.output_text.delta" => {
                    if let Some(d) = ev.get("delta").and_then(|x| x.as_str()) {
                        text.push_str(d);
                    }
                }
                "response.output_item.added" => {
                    if let Some(it) = ev.get("item") {
                        if it.get("type").and_then(|x| x.as_str()) == Some("function_call") {
                            idx += 1;
                            tools_by_idx.insert(
                                idx,
                                (
                                    it.get("call_id").and_then(|x| x.as_str()).unwrap_or("").into(),
                                    it.get("name").and_then(|x| x.as_str()).unwrap_or("").into(),
                                    String::new(),
                                ),
                            );
                        }
                    }
                }
                "response.function_call_arguments.delta" => {
                    if let Some(entry) = tools_by_idx.get_mut(&idx) {
                        if let Some(d) = ev.get("delta").and_then(|x| x.as_str()) {
                            entry.2.push_str(d);
                        }
                    }
                }
                "response.completed" => {
                    if let Some(u) = ev.get("response").and_then(|r| r.get("usage")) {
                        usage = u.clone();
                    }
                }
                _ => {}
            }
        }
    }

    let tool_calls: Vec<Value> = tools_by_idx
        .values()
        .map(|(id, name, args)| {
            json!({
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": args },
            })
        })
        .collect();
    let mut msg = json!({
        "role": "assistant",
        "content": if text.is_empty() { Value::Null } else { Value::String(text) },
    });
    if !tool_calls.is_empty() {
        msg["tool_calls"] = Value::Array(tool_calls.clone());
    }
    let input_tokens = usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
    Ok(json!({
        "id": translate::chatcmpl_id(),
        "object": "chat.completion",
        "created": translate::unix_now(),
        "model": req.get("model").cloned().unwrap_or(Value::Null),
        "choices": [{
            "index": 0, "message": msg,
            "finish_reason": if tool_calls.is_empty() { "stop" } else { "tool_calls" },
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens,
        }
    }))
}

// ---- streaming -------------------------------------------------------------

pub fn openai_stream(req: Value) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
    let s = async_stream::stream! {
        let (token, acct) = match token_and_account().await {
            Ok(p) => p,
            Err(e) => {
                yield Ok(Bytes::from(sse::data_line(&json!({
                    "error": {"message": format!("chatgpt: {}", e), "type": "upstream_error"}
                }).to_string())));
                yield Ok(Bytes::from(sse::done_line()));
                return;
            }
        };
        let body = build_body(&req, true);
        let cid = translate::chatcmpl_id();
        let created = translate::unix_now();
        let model = req.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mk_chunk = |delta: Value, finish: Option<&str>| -> String {
            sse::data_line(&json!({
                "id": cid, "object": "chat.completion.chunk", "created": created,
                "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
            }).to_string())
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
        let r = match client.post(RESPONSES).headers(headers(&token, &acct)).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                yield Ok(Bytes::from(sse::data_line(&json!({
                    "error": {"message": format!("chatgpt: {}", e), "type": "upstream_error"}
                }).to_string())));
                yield Ok(Bytes::from(sse::done_line()));
                return;
            }
        };
        if !r.status().is_success() {
            let status = r.status();
            let err = r.text().await.unwrap_or_default();
            let snip: String = err.chars().take(300).collect();
            yield Ok(Bytes::from(sse::data_line(&json!({
                "error": {"message": format!("chatgpt {}: {}", status, snip), "type": "upstream_error"}
            }).to_string())));
            yield Ok(Bytes::from(sse::done_line()));
            return;
        }
        yield Ok(Bytes::from(mk_chunk(json!({"role": "assistant", "content": ""}), None)));
        let mut tool_idx: i64 = -1;
        let mut finish = "stop";
        let mut bs = r.bytes_stream();
        let mut leftover = String::new();
        while let Some(c) = bs.next().await {
            let c = match c {
                Ok(c) => c,
                Err(_) => break,
            };
            leftover.push_str(&String::from_utf8_lossy(&c));
            while let Some(nl) = leftover.find('\n') {
                let line: String = leftover.drain(..=nl).collect();
                let line = line.trim_end_matches(['\r', '\n']).trim().to_string();
                if !line.starts_with("data:") { continue; }
                let Ok(ev): Result<Value, _> = serde_json::from_str(line[5..].trim()) else { continue; };
                let et = ev.get("type").and_then(|x| x.as_str()).unwrap_or("");
                match et {
                    "response.output_text.delta" => {
                        let d = ev.get("delta").and_then(|x| x.as_str()).unwrap_or("");
                        yield Ok(Bytes::from(mk_chunk(json!({"content": d}), None)));
                    }
                    "response.output_item.added" => {
                        if let Some(it) = ev.get("item") {
                            if it.get("type").and_then(|x| x.as_str()) == Some("function_call") {
                                tool_idx += 1;
                                finish = "tool_calls";
                                let id = it.get("call_id").and_then(|x| x.as_str()).unwrap_or("");
                                let name = it.get("name").and_then(|x| x.as_str()).unwrap_or("");
                                yield Ok(Bytes::from(mk_chunk(json!({
                                    "tool_calls": [{
                                        "index": tool_idx, "id": id, "type": "function",
                                        "function": {"name": name, "arguments": ""}
                                    }]
                                }), None)));
                            }
                        }
                    }
                    "response.function_call_arguments.delta" => {
                        let d = ev.get("delta").and_then(|x| x.as_str()).unwrap_or("");
                        yield Ok(Bytes::from(mk_chunk(json!({
                            "tool_calls": [{"index": tool_idx, "function": {"arguments": d}}]
                        }), None)));
                    }
                    "response.completed" => break,
                    _ => {}
                }
            }
        }
        yield Ok(Bytes::from(mk_chunk(json!({}), Some(finish))));
        yield Ok(Bytes::from(sse::done_line()));
    };
    Box::pin(s)
}

// ---- Responses API proxy ---------------------------------------------------

pub async fn proxy_responses(body: Value) -> Result<Value> {
    let (token, acct) = token_and_account().await?;
    let mut forward = body.clone();
    let model = resolve_model(
        forward
            .get("model")
            .and_then(|x| x.as_str())
            .unwrap_or(DEFAULT),
    );
    if let Value::Object(ref mut m) = forward {
        m.insert("model".into(), Value::String(model.clone()));
        m.insert("stream".into(), Value::Bool(true));
        if !m.contains_key("store") {
            m.insert("store".into(), Value::Bool(false));
        }
        if m.get("instructions")
            .and_then(|x| x.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            m.insert(
                "instructions".into(),
                Value::String("You are a helpful assistant.".into()),
            );
        }
        // Normalize bare string input → message-array shape (Codex requires array).
        if let Some(Value::String(s)) = m.get("input").cloned() {
            m.insert(
                "input".into(),
                json!([{
                    "type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": s}]
                }]),
            );
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let r = client
        .post(RESPONSES)
        .headers(headers(&token, &acct))
        .json(&forward)
        .send()
        .await?;
    if !r.status().is_success() {
        let status = r.status();
        let txt = r.text().await.unwrap_or_default();
        let snip: String = txt.chars().take(400).collect();
        return Err(anyhow!("chatgpt {}: {}", status, snip));
    }

    let mut text = String::new();
    let mut tools_by_idx: BTreeMap<i64, (String, String, String)> = BTreeMap::new();
    let mut idx: i64 = -1;
    let mut image_items: Vec<Value> = Vec::new();
    let mut usage = json!({});
    let mut response_id = String::new();
    let mut bs = r.bytes_stream();
    let mut leftover = String::new();

    while let Some(c) = bs.next().await {
        let c = match c {
            Ok(c) => c,
            Err(_) => break,
        };
        leftover.push_str(&String::from_utf8_lossy(&c));
        while let Some(nl) = leftover.find('\n') {
            let line: String = leftover.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']).trim().to_string();
            if !line.starts_with("data:") {
                continue;
            }
            let Ok(ev): Result<Value, _> = serde_json::from_str(line[5..].trim()) else { continue; };
            let et = ev.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match et {
                "response.created" => {
                    if let Some(id) = ev
                        .get("response")
                        .and_then(|r| r.get("id"))
                        .and_then(|x| x.as_str())
                    {
                        if !id.is_empty() {
                            response_id = id.to_string();
                        }
                    }
                }
                "response.output_text.delta" => {
                    if let Some(d) = ev.get("delta").and_then(|x| x.as_str()) {
                        text.push_str(d);
                    }
                }
                "response.output_item.added" => {
                    if let Some(it) = ev.get("item") {
                        match it.get("type").and_then(|x| x.as_str()) {
                            Some("function_call") => {
                                idx += 1;
                                tools_by_idx.insert(
                                    idx,
                                    (
                                        it.get("call_id").and_then(|x| x.as_str()).unwrap_or("").into(),
                                        it.get("name").and_then(|x| x.as_str()).unwrap_or("").into(),
                                        String::new(),
                                    ),
                                );
                            }
                            Some("image_generation_call") => {
                                image_items.push(json!({
                                    "type": "image_generation_call",
                                    "id": it.get("id").and_then(|x| x.as_str()).unwrap_or(""),
                                    "status": "in_progress",
                                    "result": Value::Null,
                                }));
                            }
                            _ => {}
                        }
                    }
                }
                "response.function_call_arguments.delta" => {
                    if let Some(entry) = tools_by_idx.get_mut(&idx) {
                        if let Some(d) = ev.get("delta").and_then(|x| x.as_str()) {
                            entry.2.push_str(d);
                        }
                    }
                }
                "response.image_generation_call.partial_image" => {
                    if let Some(b) = ev.get("partial_image_b64").and_then(|x| x.as_str()) {
                        if b.len() > 500 {
                            if let Some(last) = image_items.last_mut() {
                                last["result"] = Value::String(b.to_string());
                            }
                        }
                    }
                }
                "response.image_generation_call.completed" => {
                    if let Some(last) = image_items.last_mut() {
                        last["status"] = Value::String("completed".into());
                    }
                }
                "response.completed" => {
                    let resp = ev.get("response").cloned().unwrap_or(json!({}));
                    if let Some(u) = resp.get("usage") {
                        usage = u.clone();
                    }
                    if let Some(id) = resp.get("id").and_then(|x| x.as_str()) {
                        if !id.is_empty() {
                            response_id = id.to_string();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut output: Vec<Value> = Vec::new();
    if !text.is_empty() {
        output.push(json!({
            "type": "message", "role": "assistant",
            "content": [{"type": "output_text", "text": text}],
        }));
    }
    for (_, (id, name, args)) in tools_by_idx {
        output.push(json!({
            "type": "function_call",
            "call_id": id, "name": name, "arguments": args,
        }));
    }
    output.extend(image_items);

    let input_tokens = usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
    Ok(json!({
        "id": if response_id.is_empty() { translate::resp_id() } else { response_id },
        "object": "response",
        "created_at": translate::unix_now(),
        "model": model,
        "status": "completed",
        "output": output,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens,
        }
    }))
}

// ---- image generation ------------------------------------------------------

pub async fn generate_image(prompt: &str, _n: usize) -> Result<Vec<String>> {
    let (token, acct) = token_and_account().await?;
    let body = json!({
        "model": DEFAULT,
        "instructions": "You generate images when asked.",
        "input": [{
            "type": "message", "role": "user",
            "content": [{"type": "input_text", "text": prompt}],
        }],
        "tools": [{"type": "image_generation"}],
        "stream": true,
        "store": false,
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let r = client
        .post(RESPONSES)
        .headers(headers(&token, &acct))
        .json(&body)
        .send()
        .await?;
    if !r.status().is_success() {
        let status = r.status();
        let txt = r.text().await.unwrap_or_default();
        let snip: String = txt.chars().take(300).collect();
        return Err(anyhow!("chatgpt image {}: {}", status, snip));
    }
    let mut last_b64: Option<String> = None;
    let mut bs = r.bytes_stream();
    let mut leftover = String::new();
    while let Some(c) = bs.next().await {
        let c = match c {
            Ok(c) => c,
            Err(_) => break,
        };
        leftover.push_str(&String::from_utf8_lossy(&c));
        while let Some(nl) = leftover.find('\n') {
            let line: String = leftover.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']).trim().to_string();
            if !line.starts_with("data:") {
                continue;
            }
            let Ok(ev): Result<Value, _> = serde_json::from_str(line[5..].trim()) else { continue; };
            if let Some(b) = ev.get("partial_image_b64").and_then(|x| x.as_str()) {
                if b.len() > 500 {
                    last_b64 = Some(b.to_string());
                }
            }
        }
    }
    Ok(last_b64.map(|b| vec![b]).unwrap_or_default())
}

// ---- detect ----------------------------------------------------------------

pub async fn detect() -> Result<Vec<String>> {
    // ChatGPT Codex doesn't expose a public models list — use the static set.
    let models: Vec<String> = MODELS.iter().map(|s| s.to_string()).collect();
    let _ = store::save_detected_models("chatgpt", &models);
    Ok(models)
}
