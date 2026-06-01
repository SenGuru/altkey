//! Guards for the agent/relay-facing endpoints. The relay presents the service
//! secret (header). The agent presents its agent token (in the request body), which
//! is resolved to an account via a constant-time hash lookup.
use crate::entities::{agent, prelude::Agent};
use crate::state::AppState;
use altkey_api::token;
use axum::http::HeaderMap;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

/// True if the request carries the configured internal service secret.
pub fn service_secret_ok(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.config.internal_service_secret.as_ref() else {
        return false;
    };
    let supplied = headers
        .get("x-altkey-service-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Constant-time compare to prevent timing oracles.
    use subtle::ConstantTimeEq;
    let a = supplied.as_bytes();
    let b = expected.as_bytes();
    a.len() == b.len() && a.ct_eq(b).into()
}

/// Resolve an agent token to its (active) agent row, constant-time on the hash.
pub async fn agent_for_token(state: &AppState, agent_token: &str) -> Option<agent::Model> {
    let hash = token::hash(agent_token);
    let candidate = Agent::find()
        .filter(agent::Column::AgentTokenHash.eq(hash))
        .one(&state.db)
        .await
        .ok()??;
    if candidate.status != "active" {
        return None;
    }
    // Re-verify constant-time for defense in depth (DB lookup is by hash, but
    // verify_hash does a second constant-time hash comparison).
    if token::verify_hash(agent_token, &candidate.agent_token_hash) {
        Some(candidate)
    } else {
        None
    }
}
