//! The validation authority. `authorize` (relay) confirms a tunnel: agent token
//! valid + owns the handle + sub active → plan + limits. `key_validate` (agent)
//! confirms an ak_live_ key + the account's sub. `heartbeat` updates last_seen.
use crate::billing::store::active_subscription;
use crate::entities::{agent, endpoint_key, handle, prelude::*};
use crate::internal::auth::{agent_for_token, service_secret_ok};
use crate::state::AppState;
use altkey_api::dto::{AuthorizeRequest, AuthorizeResponse, KeyValidateRequest, KeyValidateResponse, Limits};
use altkey_api::token;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;

fn limits_for(plan: &str) -> Limits {
    match plan {
        "pro" => Limits { max_concurrency: 0, max_rps: 0 }, // unlimited
        _ => Limits { max_concurrency: 8, max_rps: 20 },     // fair cap for standard/founding
    }
}

fn deny() -> AuthorizeResponse {
    AuthorizeResponse {
        ok: false,
        account_id: String::new(),
        plan: String::new(),
        limits: limits_for(""),
    }
}

// utoipa-axum's routes!() macro requires ToSchema on request body types so the
// OpenAPI document can reference the schema. The canonical DTOs live in altkey-api
// which carries no utoipa dependency. We declare thin local schema-only structs here
// (used only in #[utoipa::path] request_body annotations) so the OpenAPI spec is
// accurate. The actual handler parameters use the altkey-api types directly.

#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct AuthorizeBody {
    handle: String,
    agent_token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct KeyValidateBody {
    key: String,
    agent_token: String,
}

/// Small body type for the heartbeat endpoint — agent_token only.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct Heartbeat {
    pub agent_token: String,
}

// ---------------------------------------------------------------------------
// Public handler functions — called directly by tests via State injection
// and registered in app.rs via routes!().
// The public API uses the altkey-api canonical DTO types so tests work with them.
// ---------------------------------------------------------------------------

/// POST /internal/agent/authorize
///
/// Called by the relay to check whether a given agent_token + handle is allowed.
/// Requires the service secret in `x-altkey-service-secret`.
#[utoipa::path(post, path = "/internal/agent/authorize", tag = "internal",
    request_body = AuthorizeBody,
    responses((status = 200, description = "Authorized or not"), (status = 401, description = "Bad service secret")))]
pub async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AuthorizeRequest>,
) -> (StatusCode, Json<AuthorizeResponse>) {
    // Service secret gate — 401 (not 200) so the relay can distinguish
    // "misconfigured secret" from "agent not authorized".
    if !service_secret_ok(&state, &headers) {
        return (StatusCode::UNAUTHORIZED, Json(deny()));
    }

    // Resolve agent token → active agent row.
    let Some(agent) = agent_for_token(&state, &req.agent_token).await else {
        return (StatusCode::OK, Json(deny()));
    };

    // Handle must exist, be active, and be owned by this agent's account.
    let Some(h) = Handle::find()
        .filter(handle::Column::Name.eq(req.handle.clone()))
        .one(&state.db)
        .await
        .ok()
        .flatten()
    else {
        return (StatusCode::OK, Json(deny()));
    };

    // Three conditions: handle is active, handle belongs to agent's account,
    // and the agent was paired to this specific handle.
    if h.status != "active"
        || h.account_id != agent.account_id
        || agent.handle_id != h.id
    {
        return (StatusCode::OK, Json(deny()));
    }

    // Subscription must be active.
    let Some(sub) = active_subscription(&state.db, agent.account_id)
        .await
        .ok()
        .flatten()
    else {
        return (StatusCode::OK, Json(deny()));
    };

    (
        StatusCode::OK,
        Json(AuthorizeResponse {
            ok: true,
            account_id: agent.account_id.to_string(),
            plan: sub.plan.clone(),
            limits: limits_for(&sub.plan),
        }),
    )
}

/// POST /internal/key/validate
///
/// Called by the agent/engine to check whether an ak_live_ key is valid and the
/// account's subscription is active.
#[utoipa::path(post, path = "/internal/key/validate", tag = "internal",
    request_body = KeyValidateBody,
    responses((status = 200, description = "Key validity + sub status")))]
pub async fn key_validate(
    State(state): State<AppState>,
    Json(req): Json<KeyValidateRequest>,
) -> Json<KeyValidateResponse> {
    let invalid = || Json(KeyValidateResponse { valid: false, sub_active: false, plan: String::new() });

    // The agent token authenticates the calling machine.
    let Some(agent) = agent_for_token(&state, &req.agent_token).await else {
        return invalid();
    };

    // The ak_live_ key must exist, be unrevoked, and belong to the agent's account.
    let hash = token::hash(&req.key);
    let Some(k) = EndpointKey::find()
        .filter(endpoint_key::Column::KeyHash.eq(hash))
        .one(&state.db)
        .await
        .ok()
        .flatten()
    else {
        return invalid();
    };

    if k.revoked_at.is_some()
        || k.account_id != agent.account_id
        || !token::verify_hash(&req.key, &k.key_hash)
    {
        return invalid();
    }

    // Best-effort last_used_at touch — ignore errors (best-effort).
    let mut am: endpoint_key::ActiveModel = k.clone().into();
    am.last_used_at = Set(Some(Utc::now().into()));
    let _ = am.update(&state.db).await;

    let sub = active_subscription(&state.db, agent.account_id)
        .await
        .ok()
        .flatten();
    match sub {
        Some(s) => Json(KeyValidateResponse { valid: true, sub_active: true, plan: s.plan }),
        None => Json(KeyValidateResponse { valid: true, sub_active: false, plan: String::new() }),
    }
}

/// Schema-only wrapper for utoipa — see AuthorizeBody note above.
#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct UsageBatchBody {
    agent_token: String,
    records: Vec<UsageRecordDtoBody>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct UsageRecordDtoBody {
    ts: String,
    provider: String,
    model: String,
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    tunnel_bytes: i64,
    tool: Option<String>,
    key_prefix: Option<String>,
}

/// POST /internal/usage
///
/// Agent-token authenticated batch ingest of usage records.
/// Unknown token → 401. Skips bad rows but never fails the batch.
#[utoipa::path(post, path = "/internal/usage", tag = "internal",
    request_body = UsageBatchBody,
    responses((status = 200, description = "Ingested"), (status = 401, description = "Unknown agent token")))]
pub async fn ingest_usage(
    State(state): State<AppState>,
    Json(batch): Json<altkey_api::dto::UsageBatch>,
) -> StatusCode {
    let Some(agent) = crate::internal::auth::agent_for_token(&state, &batch.agent_token).await else {
        return StatusCode::UNAUTHORIZED;
    };
    let _ = crate::usage::store::insert_records(&state.db, agent.account_id, Some(agent.id), &batch.records).await;
    StatusCode::OK
}

/// POST /internal/agent/heartbeat
///
/// Best-effort ping: the agent presents its token; last_seen_at is updated.
/// Unknown token still returns 200 — don't leak token existence.
#[utoipa::path(post, path = "/internal/agent/heartbeat", tag = "internal",
    request_body = Heartbeat,
    responses((status = 200, description = "ok")))]
pub async fn heartbeat(
    State(state): State<AppState>,
    Json(req): Json<Heartbeat>,
) -> StatusCode {
    if let Some(ag) = agent_for_token(&state, &req.agent_token).await {
        let mut am: agent::ActiveModel = ag.into();
        am.last_seen_at = Set(Some(Utc::now().into()));
        let _ = am.update(&state.db).await;
    }
    StatusCode::OK
}
