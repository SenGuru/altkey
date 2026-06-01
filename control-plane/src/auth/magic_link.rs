//! Email magic-link login: `request` stores a hashed single-use token and emails a
//! link; `consume` verifies it (unexpired, unused), upserts the account, and issues
//! a session cookie.
use crate::auth::{accounts, session};
use crate::entities::{magic_link, prelude::MagicLink};
use crate::error::ApiError;
use crate::state::AppState;
use altkey_api::token;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use rand::Rng;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, utoipa::ToSchema)]
pub struct MagicRequest {
    pub email: String,
}

fn random_token() -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..40).map(|_| A[rng.gen_range(0..A.len())] as char).collect()
}

#[utoipa::path(
    post, path = "/auth/magic-link/request",
    request_body = MagicRequest,
    responses((status = 200, description = "Magic link sent if the email is valid")),
    tag = "auth"
)]
pub async fn request(
    State(state): State<AppState>,
    Json(body): Json<MagicRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let email = body.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::BadRequest("invalid email".into()));
    }
    let plaintext = random_token();
    magic_link::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set(email.clone()),
        token_hash: Set(token::hash(&plaintext)),
        expires_at: Set((Utc::now() + Duration::minutes(15)).into()),
        consumed_at: Set(None),
    }
    .insert(&state.db)
    .await?;

    let link = format!("{}/auth/magic-link/consume?token={}", state.config.public_base_url, plaintext);
    state
        .email
        .send_magic_link(&email, &link)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ConsumeQuery {
    pub token: String,
}

#[utoipa::path(
    get, path = "/auth/magic-link/consume",
    params(("token" = String, Query, description = "The one-time token from the email")),
    responses((status = 303, description = "Session issued; redirect to dashboard")),
    tag = "auth"
)]
pub async fn consume(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(q): Query<ConsumeQuery>,
) -> Result<Response, ApiError> {
    let hash = token::hash(&q.token);
    let row = MagicLink::find()
        .filter(magic_link::Column::TokenHash.eq(hash))
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    if row.consumed_at.is_some() || row.expires_at < Utc::now() {
        return Err(ApiError::Unauthorized);
    }

    // Mark consumed (single-use).
    let mut am: magic_link::ActiveModel = row.clone().into();
    am.consumed_at = Set(Some(Utc::now().into()));
    am.update(&state.db).await?;

    let acct = accounts::upsert_account_with_identity(&state.db, &row.email, "magic_link", &row.email).await?;
    let token = session::issue(&state.db, acct.id).await?;
    let jar = jar.add(session::cookie_for(token));
    Ok((jar, Redirect::to("/")).into_response())
}
