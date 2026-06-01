//! Session-gated registry CRUD: handles, agents (ak_agent_), and endpoint keys (ak_live_).
//! Secrets are returned ONCE at creation; list views expose only the prefix.
use crate::auth::extract::CurrentAccount;
use crate::entities::{agent, endpoint_key, handle, prelude::*};
use crate::error::ApiError;
use crate::registry::store;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Request / response structs
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ClaimHandleRequest {
    pub name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct HandleView {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
}

impl From<handle::Model> for HandleView {
    fn from(h: handle::Model) -> Self {
        HandleView {
            id: h.id.to_string(),
            name: h.name,
            status: h.status,
            created_at: h.created_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AvailabilityResponse {
    pub available: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AvailabilityQuery {
    pub name: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateAgentRequest {
    pub handle_id: String,
    pub name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub handle_id: String,
    /// Only the first 15 characters of the token — the secret itself is never returned in list views.
    pub token_prefix: String,
    pub status: String,
    pub created_at: String,
}

impl From<agent::Model> for AgentView {
    fn from(a: agent::Model) -> Self {
        AgentView {
            id: a.id.to_string(),
            name: a.name,
            handle_id: a.handle_id.to_string(),
            token_prefix: a.token_prefix,
            status: a.status,
            created_at: a.created_at.to_rfc3339(),
        }
    }
}

/// Returned only at creation time — includes the plaintext token (shown ONCE).
#[derive(Serialize, utoipa::ToSchema)]
pub struct CreatedAgent {
    pub id: String,
    pub name: String,
    pub handle_id: String,
    pub token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateKeyRequest {
    pub name: String,
    pub agent_id: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct KeyView {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub agent_id: Option<String>,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

impl From<endpoint_key::Model> for KeyView {
    fn from(k: endpoint_key::Model) -> Self {
        KeyView {
            id: k.id.to_string(),
            name: k.name,
            key_prefix: k.key_prefix,
            agent_id: k.agent_id.map(|id| id.to_string()),
            created_at: k.created_at.to_rfc3339(),
            revoked_at: k.revoked_at.map(|t| t.to_rfc3339()),
        }
    }
}

/// Returned only at creation time — includes the plaintext secret (shown ONCE).
#[derive(Serialize, utoipa::ToSchema)]
pub struct CreatedKey {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub secret: String,
}

// ---------------------------------------------------------------------------
// Handle handlers
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/handles",
    tag = "registry",
    responses(
        (status = 200, description = "List handles for the current account"),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn list_handles(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Result<Json<Vec<HandleView>>, ApiError> {
    let rows = Handle::find()
        .filter(handle::Column::AccountId.eq(acct.id))
        .filter(handle::Column::Status.eq("active"))
        .all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(HandleView::from).collect()))
}

#[utoipa::path(
    get, path = "/handles/availability",
    tag = "registry",
    params(("name" = String, Query, description = "Handle name to check")),
    responses(
        (status = 200, description = "Availability check result", body = AvailabilityResponse),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn handle_availability(
    State(state): State<AppState>,
    _acct: CurrentAccount,
    Query(q): Query<AvailabilityQuery>,
) -> Result<Json<AvailabilityResponse>, ApiError> {
    if !store::valid_handle_name(&q.name) {
        return Ok(Json(AvailabilityResponse { available: false }));
    }
    let available = store::handle_available(&state.db, &q.name).await?;
    Ok(Json(AvailabilityResponse { available }))
}

#[utoipa::path(
    post, path = "/handles",
    tag = "registry",
    request_body = ClaimHandleRequest,
    responses(
        (status = 200, description = "Handle claimed", body = HandleView),
        (status = 400, description = "Invalid or taken handle name"),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn create_handle(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Json(body): Json<ClaimHandleRequest>,
) -> Result<Json<HandleView>, ApiError> {
    let h = store::claim_handle(&state.db, acct.id, &body.name)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(HandleView::from(h)))
}

#[utoipa::path(
    delete, path = "/handles/{id}",
    tag = "registry",
    params(("id" = String, Path, description = "Handle UUID")),
    responses(
        (status = 200, description = "Handle revoked"),
        (status = 401, description = "Not signed in"),
        (status = 404, description = "Handle not found or not owned")
    )
)]
pub async fn delete_handle(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = Handle::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.account_id != acct.id {
        return Err(ApiError::NotFound);
    }
    let mut am: handle::ActiveModel = row.into();
    am.status = Set("revoked".into());
    am.update(&state.db).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Agent handlers
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/agents",
    tag = "registry",
    responses(
        (status = 200, description = "List agents for the current account"),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn list_agents(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Result<Json<Vec<AgentView>>, ApiError> {
    let rows = Agent::find()
        .filter(agent::Column::AccountId.eq(acct.id))
        .filter(agent::Column::Status.eq("active"))
        .all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(AgentView::from).collect()))
}

#[utoipa::path(
    post, path = "/agents",
    tag = "registry",
    request_body = CreateAgentRequest,
    responses(
        (status = 200, description = "Agent paired; token returned once", body = CreatedAgent),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Not signed in"),
        (status = 404, description = "Handle not found or not owned")
    )
)]
pub async fn create_agent(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Json(body): Json<CreateAgentRequest>,
) -> Result<Json<CreatedAgent>, ApiError> {
    let handle_id = body
        .handle_id
        .parse::<Uuid>()
        .map_err(|_| ApiError::BadRequest("invalid handle_id".into()))?;
    let paired = store::pair_agent(&state.db, acct.id, handle_id, &body.name)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not owned") || msg.contains("no such handle") {
                ApiError::NotFound
            } else {
                ApiError::BadRequest(msg)
            }
        })?;
    Ok(Json(CreatedAgent {
        id: paired.agent.id.to_string(),
        name: paired.agent.name,
        handle_id: paired.agent.handle_id.to_string(),
        token: paired.token_plaintext,
    }))
}

#[utoipa::path(
    delete, path = "/agents/{id}",
    tag = "registry",
    params(("id" = String, Path, description = "Agent UUID")),
    responses(
        (status = 200, description = "Agent revoked"),
        (status = 401, description = "Not signed in"),
        (status = 404, description = "Agent not found or not owned")
    )
)]
pub async fn delete_agent(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = Agent::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.account_id != acct.id {
        return Err(ApiError::NotFound);
    }
    let mut am: agent::ActiveModel = row.into();
    am.status = Set("revoked".into());
    am.update(&state.db).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Endpoint key handlers
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/keys",
    tag = "registry",
    responses(
        (status = 200, description = "List endpoint keys for the current account (prefix only, never secret)"),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn list_keys(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Result<Json<Vec<KeyView>>, ApiError> {
    let rows = EndpointKey::find()
        .filter(endpoint_key::Column::AccountId.eq(acct.id))
        .filter(endpoint_key::Column::RevokedAt.is_null())
        .all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(KeyView::from).collect()))
}

#[utoipa::path(
    post, path = "/keys",
    tag = "registry",
    request_body = CreateKeyRequest,
    responses(
        (status = 200, description = "Key minted; secret returned once", body = CreatedKey),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn create_key(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Json(body): Json<CreateKeyRequest>,
) -> Result<Json<CreatedKey>, ApiError> {
    let agent_id = body
        .agent_id
        .map(|s| s.parse::<Uuid>().map_err(|_| ApiError::BadRequest("invalid agent_id".into())))
        .transpose()?;
    let minted = store::mint_key(&state.db, acct.id, agent_id, &body.name)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(CreatedKey {
        id: minted.key.id.to_string(),
        name: minted.key.name,
        prefix: minted.key.key_prefix,
        secret: minted.key_plaintext,
    }))
}

#[utoipa::path(
    delete, path = "/keys/{id}",
    tag = "registry",
    params(("id" = String, Path, description = "Key UUID")),
    responses(
        (status = 200, description = "Key revoked"),
        (status = 401, description = "Not signed in"),
        (status = 404, description = "Key not found or not owned")
    )
)]
pub async fn delete_key(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = EndpointKey::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.account_id != acct.id {
        return Err(ApiError::NotFound);
    }
    let mut am: endpoint_key::ActiveModel = row.into();
    am.revoked_at = Set(Some(Utc::now().into()));
    am.update(&state.db).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
