//! Axum extractor that resolves the session cookie to the current account, or 401.
use crate::auth::session::{account_for, SESSION_COOKIE};
use crate::entities::account;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum_extra::extract::CookieJar;

pub struct CurrentAccount(pub account::Model);

#[axum::async_trait]
impl FromRequestParts<AppState> for CurrentAccount {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get(SESSION_COOKIE).map(|c| c.value().to_string());
        let Some(token) = token else { return Err(StatusCode::UNAUTHORIZED) };
        match account_for(&state.db, &token).await {
            Ok(Some(acct)) => Ok(CurrentAccount(acct)),
            Ok(None) => Err(StatusCode::UNAUTHORIZED),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}
