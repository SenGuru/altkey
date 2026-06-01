//! Adapter catalog + delivery routes.
//!
//! Session-gated dashboard view:
//!   GET /adapters           → catalog list (no manifests, metadata only)
//!
//! Unauthenticated agent delivery (manifests are not secret):
//!   GET /internal/adapters          → all adapters with manifests
//!   GET /internal/adapters/:slug    → one adapter manifest by slug
use crate::auth::extract::CurrentAccount;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Catalog row (metadata only — no manifest). Served on the session-gated
/// `/adapters` dashboard endpoint.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AdapterView {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub target_tool: Option<String>,
}

impl From<crate::entities::adapter::Model> for AdapterView {
    fn from(a: crate::entities::adapter::Model) -> Self {
        AdapterView {
            id: a.id.to_string(),
            slug: a.slug,
            name: a.name,
            description: a.description,
            version: a.version,
            target_tool: a.target_tool,
        }
    }
}

/// Full adapter row including the manifest JSON. Returned by the `/internal`
/// delivery endpoints — manifests describe public shims and are not secret.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AdapterManifestView {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub target_tool: Option<String>,
    pub manifest: serde_json::Value,
}

impl From<crate::entities::adapter::Model> for AdapterManifestView {
    fn from(a: crate::entities::adapter::Model) -> Self {
        AdapterManifestView {
            id: a.id.to_string(),
            slug: a.slug,
            name: a.name,
            description: a.description,
            version: a.version,
            target_tool: a.target_tool,
            manifest: a.manifest,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /adapters
///
/// Session-gated adapter catalog. Returns metadata rows without manifests.
#[utoipa::path(
    get,
    path = "/adapters",
    tag = "adapters",
    responses(
        (status = 200, description = "Adapter catalog", body = Vec<AdapterView>),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn list_adapters(
    State(state): State<AppState>,
    _account: CurrentAccount,
) -> Json<Vec<AdapterView>> {
    let rows = crate::adapters::store::list(&state.db)
        .await
        .unwrap_or_default();
    Json(rows.into_iter().map(AdapterView::from).collect())
}

/// GET /internal/adapters
///
/// Unauthenticated adapter delivery — returns all adapters with their manifests.
/// Manifests describe public shims and are not considered secret.
pub async fn internal_list_adapters(
    State(state): State<AppState>,
) -> Json<Vec<AdapterManifestView>> {
    let rows = crate::adapters::store::list(&state.db)
        .await
        .unwrap_or_default();
    Json(rows.into_iter().map(AdapterManifestView::from).collect())
}

/// GET /internal/adapters/:slug
///
/// Unauthenticated manifest delivery for a single adapter by slug.
pub async fn internal_get_adapter(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<AdapterManifestView>, StatusCode> {
    match crate::adapters::store::get(&state.db, &slug).await {
        Ok(Some(a)) => Ok(Json(AdapterManifestView::from(a))),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
