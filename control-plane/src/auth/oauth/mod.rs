//! Generic OAuth2 authorization-code + PKCE login. A `Provider` describes one
//! identity provider; `start` redirects to it (storing CSRF state + PKCE verifier),
//! `callback` exchanges the code, fetches the user's email + id, upserts the
//! account, and issues a session. Apple overrides userinfo (see apple.rs).
pub mod apple;
pub mod providers;

use crate::auth::{accounts, session};
use crate::entities::{oauth_flow, prelude::OauthFlow};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Form, Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// All the configuration needed to drive one OAuth2 identity provider.
#[derive(Clone)]
pub struct Provider {
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scopes: Vec<String>,
    /// Top-level JSON field name for the provider's user ID in the userinfo response.
    pub id_field: String,
    /// Top-level JSON field name for the user's email in the userinfo response.
    pub email_field: String,
}

/// Registry of configured OAuth2 providers, keyed by provider name.
#[derive(Clone, Default)]
pub struct OAuthRegistry {
    pub providers: HashMap<String, Provider>,
}

impl OAuthRegistry {
    pub fn get(&self, name: &str) -> Option<&Provider> {
        self.providers.get(name)
    }
}

/// Query params for the GET callback (Google/Microsoft/GitHub).
#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

/// `GET /auth/{provider}/start` — build the authorize URL + PKCE, store the
/// transient state row in the DB, and redirect the user's browser to the provider.
pub async fn start(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Response, ApiError> {
    let p = state.oauth.get(&provider).ok_or(ApiError::NotFound)?.clone();

    // TODO(deploy): apple needs response_mode=form_post in authorize URL when the
    // email scope is requested (Apple POSTs the callback instead of redirecting).
    // The POST /auth/apple/callback route handles Apple's form_post response.
    // When deploying, append &response_mode=form_post to the authorize URL for apple,
    // e.g. by post-processing the URL string returned by providers::authorize_url.

    let (url, csrf, verifier) = providers::authorize_url(&p);
    oauth_flow::ActiveModel {
        state: Set(csrf),
        provider: Set(provider),
        pkce_verifier: Set(verifier),
        return_to: Set(None),
        expires_at: Set((Utc::now() + Duration::minutes(10)).into()),
    }
    .insert(&state.db)
    .await?;
    Ok(Redirect::to(&url).into_response())
}

/// Shared logic for completing an OAuth callback: validate state, exchange code,
/// fetch (uid, email), upsert account + identity, issue session cookie.
async fn complete_callback(
    state: &AppState,
    provider: String,
    jar: CookieJar,
    code: String,
    state_param: String,
) -> Result<Response, ApiError> {
    let p = state.oauth.get(&provider).ok_or(ApiError::NotFound)?.clone();

    let flow = OauthFlow::find()
        .filter(oauth_flow::Column::State.eq(state_param.clone()))
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    if flow.provider != provider || flow.expires_at < Utc::now() {
        return Err(ApiError::Unauthorized);
    }

    // One-time use: delete the flow row immediately so the state can't be replayed.
    OauthFlow::delete_by_id(state_param)
        .exec(&state.db)
        .await?;

    let (uid, email) = if provider == "apple" {
        apple::exchange_and_extract(&p, &code, &flow.pkce_verifier)
            .await
            .map_err(ApiError::Internal)?
    } else {
        providers::exchange_and_fetch_userinfo(&p, &code, &flow.pkce_verifier)
            .await
            .map_err(ApiError::Internal)?
    };

    let acct =
        accounts::upsert_account_with_identity(&state.db, &email, &provider, &uid).await?;
    let token = session::issue(&state.db, acct.id).await?;
    let jar = jar.add(session::cookie_for(token));
    Ok((jar, Redirect::to("/")).into_response())
}

/// `GET /auth/{provider}/callback` — validate the CSRF state, exchange the
/// authorization code (with PKCE), fetch the user's email + id from the provider's
/// userinfo endpoint, upsert the account + identity rows, and issue a session cookie.
/// Serves Google, Microsoft, and GitHub (which redirect with query params).
pub async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    jar: CookieJar,
    Query(q): Query<CallbackQuery>,
) -> Result<Response, ApiError> {
    complete_callback(&state, provider, jar, q.code, q.state).await
}

/// `POST /auth/apple/callback` — Apple uses `response_mode=form_post` and sends
/// `code` + `state` as form fields in the POST body, not query params. This handler
/// is mounted at the fixed path `/auth/apple/callback` (no `:provider` path param)
/// and always uses the "apple" provider name.
pub async fn callback_form(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(q): Form<CallbackQuery>,
) -> Result<Response, ApiError> {
    complete_callback(&state, "apple".to_string(), jar, q.code, q.state).await
}

/// Build the provider registry from environment variables. Only providers whose
/// `{UPPER}_CLIENT_ID` and `{UPPER}_CLIENT_SECRET` are set are included.
pub fn registry_from_env() -> OAuthRegistry {
    let mut providers = HashMap::new();
    for p in providers::from_env() {
        providers.insert(p.name.clone(), p);
    }
    if let Some(a) = apple::from_env() {
        providers.insert(a.name.clone(), a);
    }
    OAuthRegistry { providers }
}

/// Convenience type alias for a shared (Arc-wrapped) registry.
pub type SharedRegistry = Arc<OAuthRegistry>;
