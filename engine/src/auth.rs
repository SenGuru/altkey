//! Bearer-key auth for /v1/* and admin-token auth for /admin/* — mirrors Python.
use axum::http::{HeaderMap, StatusCode};

use crate::{config, store};

/// Returns the validated API key, or an error tuple suitable for a 401 response.
pub fn require_key(headers: &HeaderMap) -> Result<String, (StatusCode, &'static str)> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let key = if let Some(rest) = auth.strip_prefix("Bearer ") {
        rest.trim()
    } else {
        auth.trim()
    };
    if key.is_empty() {
        return Err((StatusCode::UNAUTHORIZED, "missing api key"));
    }
    if !store::key_exists(key) {
        return Err((StatusCode::UNAUTHORIZED, "invalid api key"));
    }
    Ok(key.to_string())
}

pub fn require_admin(headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
    let Some(tok) = config::admin_token() else {
        return Ok(()); // local mode — open
    };
    let supplied = headers
        .get("x-admin-token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if supplied == tok {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "admin token required"))
    }
}
