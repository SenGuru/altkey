//! axum router — wires every endpoint Phase 0 exposed. Endpoint shapes are
//! kept bit-for-bit identical to the Python reference so OpenAI/Anthropic SDK
//! clients see no diff between the engines.
use axum::{
    body::Body,
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::providers;
use crate::providers::claude_oauth;
use crate::providers::chatgpt;
use crate::sse;
use crate::store;
use crate::{auth, config};

// Same HTML the Python reference serves. Embedded at compile time so the
// binary is self-contained; falls back to the dashboard.html on the Python
// side at build time. We point this at the local dashboard.html now to keep
// the local/admin UI working unchanged.
const DASHBOARD_HTML: &str =
    include_str!("../../local/app/dashboard.html");

pub fn build_router() -> Router {
    // CORS for the local dashboard — same regex as Python's main.py.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            let s = origin.to_str().unwrap_or("");
            // http(s)://127.0.0.1 or http(s)://localhost (optional port)
            let re_check = |scheme: &str| {
                let prefixes = [
                    format!("{}://127.0.0.1", scheme),
                    format!("{}://localhost", scheme),
                ];
                prefixes.iter().any(|p| {
                    s == p
                        || s.starts_with(&format!("{}:", p))
                        || s.starts_with(&format!("{}/", p))
                })
            };
            re_check("http") || re_check("https")
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    // Best-effort seed of cached Claude OAuth from ~/.claude/.credentials.json
    claude_oauth::cache_from_file();

    Router::new()
        .route("/", get(dashboard))
        .route("/callback", get(oauth_callback))
        .route("/v1/models", get(v1_models))
        .route("/v1/chat/completions", post(v1_chat))
        .route("/v1/messages", post(v1_messages))
        .route("/v1/responses", post(v1_responses))
        .route("/v1/images/generations", post(v1_images))
        .route("/admin/status", get(admin_status))
        .route("/admin/disconnect", post(admin_disconnect))
        .route("/admin/connect-cli", post(admin_connect_cli))
        .route("/admin/oauth/start", post(admin_oauth_start))
        .route("/admin/oauth/finish", post(admin_oauth_finish))
        .route("/admin/detect", post(admin_detect))
        .route("/admin/keys", post(admin_keys))
        .route("/admin/keys/revoke", post(admin_keys_revoke))
        .route("/admin/transparent/enable", post(admin_transparent_enable))
        .route("/admin/transparent/disable", post(admin_transparent_disable))
        .route("/admin/transparent/status", get(admin_transparent_status))
        .route("/admin/tunnel/start", post(admin_tunnel_start))
        .route("/admin/tunnel/stop", post(admin_tunnel_stop))
        .route("/admin/tunnel/status", get(admin_tunnel_status))
        .layer(cors)
}

// ---- auth helper -----------------------------------------------------------

/// Validate the presented API key. Local-store check first (unchanged historical
/// behavior); then, when the control plane is configured (`CONTROL_PLANE_URL` +
/// `ALTKEY_AGENT_TOKEN`), the key must ALSO pass the control-plane gate. When the
/// control plane is unconfigured the gate is a no-op, so the default path and all
/// existing tests are unaffected.
async fn auth_key(headers: &HeaderMap) -> Result<String, (StatusCode, Json<Value>)> {
    let transparent = std::env::var("ALTKEY_TRANSPARENT").as_deref() == Ok("1");
    let from_bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let from_xapi = headers
        .get("x-api-key")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let key = if !from_bearer.is_empty() { from_bearer } else { from_xapi };
    if transparent {
        return Ok(key);
    }
    if key.is_empty() {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"detail": "missing api key"}))));
    }
    if !store::key_exists(&key) {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"detail": "invalid api key"}))));
    }
    // Control-plane gate: no-op when unconfigured, otherwise must approve the key.
    if let Err((s, msg)) = auth::control_plane_ok(&key).await {
        return Err((s, Json(json!({"detail": msg}))));
    }
    Ok(key)
}

fn require_admin(headers: &HeaderMap) -> Result<(), (StatusCode, Json<Value>)> {
    auth::require_admin(headers).map_err(|(s, msg)| (s, Json(json!({"detail": msg}))))
}

fn err502(msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_GATEWAY, Json(json!({"detail": format!("upstream error: {}", msg.into())})))
}

// ---- dashboard + oauth callback -------------------------------------------

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

#[derive(Deserialize)]
struct CallbackParams {
    #[serde(default)]
    code: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    error: String,
}

async fn oauth_callback(Query(p): Query<CallbackParams>) -> Html<String> {
    if !p.error.is_empty() {
        return Html(format!(
            "<h2>Connect failed</h2><p>{}</p><p>Close this tab and try again.</p>",
            p.error
        ));
    }
    if p.code.is_empty() {
        return Html("<h2>Missing code</h2><p>Close this tab and try again.</p>".to_string());
    }
    let combined = if p.state.is_empty() {
        p.code.clone()
    } else {
        format!("{}#{}", p.code, p.state)
    };
    match claude_oauth::finish_oauth(&combined).await {
        Ok(res) => {
            let sub = res.get("subscription").and_then(|x| x.as_str()).unwrap_or("?");
            tokio::spawn(async { let _ = providers::detect_one("claude_oauth").await; });
            Html(format!(
                "<h2>Claude connected</h2><p>Subscription: <b>{}</b>. Tool calling enabled.</p>\
                <p>You can close this tab and return to altkey.</p>\
                <script>setTimeout(()=>window.close(),2500)</script>",
                sub
            ))
        }
        Err(e) => Html(format!(
            "<h2>Connect failed</h2><pre>{}</pre><p>Close this tab and try again.</p>",
            e.to_string().chars().take(400).collect::<String>()
        )),
    }
}

// ---- /v1/models ------------------------------------------------------------

async fn v1_models(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = auth_key(&headers).await?;
    let scope = store::key_provider(&key);
    let mut models = providers::list_models();
    if let Some(s) = scope.as_deref() {
        models.retain(|m| m.get("owned_by").and_then(|x| x.as_str()) == Some(s));
    }
    Ok(Json(json!({"object": "list", "data": models})))
}

// ---- /v1/chat/completions --------------------------------------------------

async fn v1_chat(headers: HeaderMap, body: Json<Value>) -> Response {
    let key = match auth_key(&headers).await {
        Ok(k) => k,
        Err(e) => return (e.0, e.1).into_response(),
    };
    let body = body.0;
    let model = body
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("claude-sonnet-4-5")
        .to_string();
    let prov = match providers::for_model(&model) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"detail": format!("unknown model: {}", model)}))).into_response(),
    };
    let scope = store::key_provider(&key);
    if let Some(s) = scope.as_deref() {
        if prov.name() != s {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"detail": format!("this key is scoped to '{}' and cannot use model '{}' ({})", s, model, prov.name())})),
            )
                .into_response();
        }
    }
    let want_stream = body.get("stream").and_then(|x| x.as_bool()).unwrap_or(false);

    if want_stream {
        let stream = match prov {
            providers::Provider::Claude => claude_oauth::openai_stream(body),
            providers::Provider::Chatgpt => chatgpt::openai_stream(body),
        };
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(stream))
            .unwrap();
    }

    let result = match prov {
        providers::Provider::Claude => claude_oauth::openai_completion(&body).await,
        providers::Provider::Chatgpt => chatgpt::openai_completion(&body).await,
    };
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => err502(e.to_string()).into_response(),
    }
}

// ---- /v1/messages (Anthropic passthrough) ---------------------------------

async fn v1_messages(headers: HeaderMap, body: Json<Value>) -> Response {
    let key = match auth_key(&headers).await {
        Ok(k) => k,
        Err(e) => return (e.0, e.1).into_response(),
    };
    let scope = store::key_provider(&key);
    if let Some(s) = scope.as_deref() {
        if s != "claude" {
            return (StatusCode::FORBIDDEN, Json(json!({"detail": format!("this key is scoped to '{}', not claude", s)}))).into_response();
        }
    }
    if !claude_oauth::is_connected() {
        return (StatusCode::BAD_REQUEST, Json(json!({"detail": "claude (oauth) not connected — run Connect Claude (CLI)"}))).into_response();
    }
    let body = body.0;
    let want_stream = body.get("stream").and_then(|x| x.as_bool()).unwrap_or(false);
    if want_stream {
        let stream = claude_oauth::anthropic_messages_stream(body);
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(stream))
            .unwrap();
    }
    match claude_oauth::anthropic_messages(body).await {
        Ok((status, val)) => {
            // Convert reqwest::StatusCode → axum::StatusCode via u16
            let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);
            (code, Json(val)).into_response()
        }
        Err(e) => err502(e.to_string()).into_response(),
    }
}

// ---- /v1/responses ---------------------------------------------------------

async fn v1_responses(headers: HeaderMap, body: Json<Value>) -> Response {
    if let Err(e) = auth_key(&headers).await {
        return (e.0, e.1).into_response();
    }
    match chatgpt::proxy_responses(body.0).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err502(e.to_string()).into_response(),
    }
}

// ---- /v1/images/generations -----------------------------------------------

async fn v1_images(headers: HeaderMap, body: Json<Value>) -> Response {
    if let Err(e) = auth_key(&headers).await {
        return (e.0, e.1).into_response();
    }
    let prompt = body.0.get("prompt").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if prompt.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"detail": "missing prompt"}))).into_response();
    }
    let n = body.0.get("n").and_then(|x| x.as_i64()).unwrap_or(1) as usize;
    match chatgpt::generate_image(&prompt, n).await {
        Ok(images) if !images.is_empty() => Json(json!({
            "created": crate::translate::unix_now(),
            "data": images.iter().map(|b| json!({"b64_json": b})).collect::<Vec<_>>(),
        }))
        .into_response(),
        Ok(_) => err502("no image returned").into_response(),
        Err(e) => err502(format!("image generation error: {}", e)).into_response(),
    }
}

// ---- /admin/* -------------------------------------------------------------

async fn admin_status(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    let keys: Vec<Value> = store::list_keys()
        .into_iter()
        .map(|k| {
            json!({
                "key": k.key,
                "label": k.label.unwrap_or_default(),
                "created_at": k.created_at,
                "provider": k.provider,
            })
        })
        .collect();
    Ok(Json(json!({
        "sessions": providers::live_sessions(),
        "keys": keys,
    })))
}

#[derive(Deserialize)]
struct ProviderReq {
    provider: String,
}

async fn admin_disconnect(
    headers: HeaderMap,
    Json(body): Json<ProviderReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    let _ = store::delete_session(&body.provider);
    Ok(Json(json!({"ok": true})))
}

async fn admin_connect_cli(
    headers: HeaderMap,
    Json(body): Json<ProviderReq>,
) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    match body.provider.as_str() {
        "claude" => {
            let p = config::claude_creds_path();
            if !p.exists() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"ok": false, "error": "no Claude Code credentials found — log into Claude Code first"})),
                )
                    .into_response();
            }
            claude_oauth::cache_from_file();
            tokio::spawn(async { let _ = providers::detect_one("claude_oauth").await; });
            Json(json!({"ok": true, "provider": "claude", "mode": "oauth"})).into_response()
        }
        "chatgpt" => {
            let p = config::codex_creds_path();
            if !p.exists() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"ok": false, "error": "no Codex CLI credentials found — run `codex` and log in first"})),
                )
                    .into_response();
            }
            // is_connected() returns true if file exists; no extra step needed.
            tokio::spawn(async { let _ = providers::detect_one("chatgpt").await; });
            Json(json!({"ok": true, "provider": "chatgpt", "mode": "oauth"})).into_response()
        }
        other => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": format!("unsupported provider: {}", other)})),
        )
            .into_response(),
    }
}

async fn admin_oauth_start(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    Ok(Json(claude_oauth::start_oauth()))
}

#[derive(Deserialize)]
struct OAuthFinishReq {
    code: String,
}

async fn admin_oauth_finish(
    headers: HeaderMap,
    Json(body): Json<OAuthFinishReq>,
) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    match claude_oauth::finish_oauth(&body.code).await {
        Ok(v) => {
            tokio::spawn(async { let _ = providers::detect_one("claude_oauth").await; });
            Json(v).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn admin_detect(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    Ok(Json(json!({"ok": true, "detected": providers::detect_all().await})))
}

#[derive(Deserialize)]
struct MintReq {
    #[serde(default)]
    label: String,
    #[serde(default)]
    provider: Option<String>,
}

async fn admin_keys(
    headers: HeaderMap,
    Json(body): Json<MintReq>,
) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    let prov = body.provider.as_deref();
    if let Some(p) = prov {
        if p != "claude" && p != "chatgpt" {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "error": format!("invalid provider scope: {}", p)})),
            )
                .into_response();
        }
    }
    match store::mint_key(&body.label, prov) {
        Ok(key) => Json(json!({"key": key, "provider": prov})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct RevokeReq {
    key: String,
}

async fn admin_keys_revoke(
    headers: HeaderMap,
    Json(body): Json<RevokeReq>,
) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    let _ = store::revoke_key(&body.key);
    Json(json!({"ok": true})).into_response()
}

// ---- /admin/transparent/* --------------------------------------------------

async fn admin_transparent_enable(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    let app = build_router();
    match crate::transparent::enable(app).await {
        Ok(()) => Json(json!({"ok": true, "transparent": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string(),
                "hint": "transparent mode needs admin/root for hosts + :443 + trust store"})),
        )
            .into_response(),
    }
}

async fn admin_transparent_disable(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    match crate::transparent::disable() {
        Ok(()) => Json(json!({"ok": true, "transparent": false})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}

async fn admin_transparent_status(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    Ok(Json(json!({"transparent": crate::transparent::is_enabled()})))
}

// ---- /admin/tunnel/* -------------------------------------------------------

async fn admin_tunnel_start(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    if crate::tunnel::is_up() {
        return Json(json!({"ok": false, "error": "tunnel already running"})).into_response();
    }
    let app = build_router();
    let relay = config::relay_addr();
    let handle = config::handle();
    tokio::spawn(async move {
        if let Err(e) = crate::tunnel::run(app, relay, handle).await {
            tracing::warn!("tunnel exited: {e}");
        }
    });
    Json(json!({"ok": true, "starting": true})).into_response()
}

async fn admin_tunnel_stop(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    // NOTE: MVP — this only clears the status flag; the spawned tunnel task keeps
    // running until its control connection drops. TODO: track the JoinHandle and
    // abort() it here to truly stop the tunnel.
    crate::tunnel::TUNNEL_UP.store(false, std::sync::atomic::Ordering::SeqCst);
    Json(json!({"ok": true, "flag_cleared": true})).into_response()
}

async fn admin_tunnel_status(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    Ok(Json(json!({"tunnel_up": crate::tunnel::is_up(), "handle": config::handle()})))
}

// Silence unused warnings in builds where a feature isn't exercised.
#[allow(dead_code)]
fn _suppress() {
    let _ = sse::done_line();
    let _ = HashMap::<String, String>::new();
}
