//! `/me` returns the current account (or 401); `/auth/logout` clears the session.
use crate::auth::extract::CurrentAccount;
use crate::auth::session::{self, SESSION_COOKIE};
use crate::state::AppState;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct Me {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
}

#[utoipa::path(
    get, path = "/me",
    responses(
        (status = 200, description = "The current account", body = Me),
        (status = 401, description = "Not signed in")
    ),
    tag = "auth"
)]
pub async fn me(CurrentAccount(acct): CurrentAccount) -> Json<Me> {
    Json(Me {
        id: acct.id.to_string(),
        email: acct.email,
        display_name: acct.display_name,
    })
}

#[utoipa::path(
    post, path = "/auth/logout",
    responses((status = 200, description = "Session cleared")),
    tag = "auth"
)]
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let _ = session::revoke(&state.db, c.value()).await;
    }
    let jar = jar.add(session::clear_cookie());
    (jar, Json(serde_json::json!({ "ok": true }))).into_response()
}
