//! Session-gated dashboard usage reads.
//! GET /usage/summary  — triggers idempotent rollup, returns per-account rollup rows.
//! GET /usage/records  — returns recent raw usage records (newest-first, capped at 200, no secrets).
use crate::auth::extract::CurrentAccount;
use crate::entities::{prelude::*, usage_record, usage_rollup};
use crate::state::AppState;
use crate::usage::rollup::rebuild_for_account;
use axum::extract::{Query, State};
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
pub struct RollupView {
    pub period: String,
    pub model: Option<String>,
    pub tool: Option<String>,
    pub provider: Option<String>,
    pub sum_requests: i64,
    pub sum_tokens: i64,
    pub sum_bytes: i64,
}

impl From<usage_rollup::Model> for RollupView {
    fn from(r: usage_rollup::Model) -> Self {
        RollupView {
            period: r.period,
            model: r.model,
            tool: r.tool,
            provider: r.provider,
            sum_requests: r.sum_requests,
            sum_tokens: r.sum_tokens,
            sum_bytes: r.sum_bytes,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RecordView {
    pub id: String,
    pub ts: String,
    pub provider: String,
    pub model: String,
    pub tool: Option<String>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub tunnel_bytes: i64,
    // key_prefix exposed (it's already a prefix, not secret), agent_id exposed as string
    pub key_prefix: Option<String>,
}

impl From<usage_record::Model> for RecordView {
    fn from(r: usage_record::Model) -> Self {
        RecordView {
            id: r.id.to_string(),
            ts: chrono::DateTime::<chrono::Utc>::from(r.ts).to_rfc3339(),
            provider: r.provider,
            model: r.model,
            tool: r.tool,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            total_tokens: r.total_tokens,
            tunnel_bytes: r.tunnel_bytes,
            key_prefix: r.key_prefix,
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RecordsQuery {
    pub limit: Option<u64>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /usage/summary
///
/// Rebuilds (idempotent) and returns the account's usage rollup rows.
/// Session-gated.
#[utoipa::path(
    get,
    path = "/usage/summary",
    tag = "usage",
    responses(
        (status = 200, description = "Rollup rows", body = Vec<RollupView>),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn usage_summary(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Json<Vec<RollupView>> {
    // Best-effort rebuild — ignore rollup errors (dashboard still returns rows).
    let _ = rebuild_for_account(&state.db, acct.id).await;

    let rows = UsageRollup::find()
        .filter(usage_rollup::Column::AccountId.eq(acct.id))
        .all(&state.db)
        .await
        .unwrap_or_default();

    Json(rows.into_iter().map(RollupView::from).collect())
}

/// GET /usage/records
///
/// Returns recent raw usage records for the current account, newest-first.
/// `limit` query param capped at 200.
/// Session-gated; no secrets in the response.
#[utoipa::path(
    get,
    path = "/usage/records",
    tag = "usage",
    params(
        ("limit" = Option<u64>, Query, description = "Max rows to return (capped at 200)")
    ),
    responses(
        (status = 200, description = "Usage records", body = Vec<RecordView>),
        (status = 401, description = "Not signed in")
    )
)]
pub async fn usage_records(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Query(q): Query<RecordsQuery>,
) -> Json<Vec<RecordView>> {
    let cap = q.limit.unwrap_or(50).min(200);

    let rows = UsageRecord::find()
        .filter(usage_record::Column::AccountId.eq(acct.id))
        .order_by_desc(usage_record::Column::Ts)
        .limit(cap)
        .all(&state.db)
        .await
        .unwrap_or_default();

    Json(rows.into_iter().map(RecordView::from).collect())
}
