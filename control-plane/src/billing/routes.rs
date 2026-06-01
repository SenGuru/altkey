//! /billing/checkout (start a Polar checkout), /billing/portal (manage), and
//! /billing/subscription (current state) — all session-authenticated.
use crate::auth::extract::CurrentAccount;
use crate::billing::plan::Plan;
use crate::billing::store::active_subscription;
use crate::entities::{prelude::Subscription, subscription};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CheckoutRequest {
    pub plan: Plan,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UrlResponse {
    pub url: String,
}

#[utoipa::path(post, path = "/billing/checkout", tag = "billing",
    request_body = CheckoutRequest,
    responses((status = 200, body = UrlResponse), (status = 401, description = "Not signed in")))]
pub async fn checkout(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Json(body): Json<CheckoutRequest>,
) -> Result<Json<UrlResponse>, ApiError> {
    let success = format!("{}/dashboard?checkout=success", state.config.public_base_url);
    let url = state.polar.create_checkout(acct.id, body.plan, &success).await
        .map_err(ApiError::Internal)?;
    Ok(Json(UrlResponse { url }))
}

#[utoipa::path(post, path = "/billing/portal", tag = "billing",
    responses((status = 200, body = UrlResponse), (status = 401), (status = 404, description = "No subscription")))]
pub async fn portal(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Result<Json<UrlResponse>, ApiError> {
    let sub = Subscription::find()
        .filter(subscription::Column::AccountId.eq(acct.id))
        .one(&state.db).await?
        .ok_or(ApiError::NotFound)?;
    let cust = sub.polar_customer_id.ok_or(ApiError::NotFound)?;
    let url = state.polar.customer_portal_url(&cust).await
        .map_err(ApiError::Internal)?;
    Ok(Json(UrlResponse { url }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SubscriptionView {
    pub plan: Option<String>,
    pub status: String,
    pub active: bool,
    pub current_period_end: Option<String>,
    pub is_founding: bool,
}

#[utoipa::path(get, path = "/billing/subscription", tag = "billing",
    responses((status = 200, body = SubscriptionView), (status = 401)))]
pub async fn subscription(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Result<Json<SubscriptionView>, ApiError> {
    let active = active_subscription(&state.db, acct.id).await?;
    let row = Subscription::find()
        .filter(subscription::Column::AccountId.eq(acct.id))
        .one(&state.db).await?;
    Ok(Json(match row {
        Some(s) => SubscriptionView {
            plan: Some(s.plan),
            status: s.status,
            active: active.is_some(),
            current_period_end: s.current_period_end.map(|d| d.to_rfc3339()),
            is_founding: s.is_founding,
        },
        None => SubscriptionView {
            plan: None,
            status: "none".into(),
            active: false,
            current_period_end: None,
            is_founding: false,
        },
    }))
}
