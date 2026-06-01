//! Bearer-key auth for /v1/* and admin-token auth for /admin/* — mirrors Python.
//!
//! Two-mode key authorization:
//!  - **Unconfigured (default):** when either `CONTROL_PLANE_URL` or
//!    `ALTKEY_AGENT_TOKEN` is unset, key validation is purely local — exactly the
//!    historical behavior (`store::key_exists`). All existing engine tests exercise
//!    this path and stay green.
//!  - **Configured:** when BOTH env vars are set, an `ak_live_` key is additionally
//!    gated by the control plane: it is served only if the control plane reports
//!    `valid && sub_active` (within the validator's 60s cache / 72h offline grace),
//!    otherwise the request is rejected. The control-plane validator is a
//!    process-global initialized once at first use.
use std::sync::OnceLock;

use axum::http::{HeaderMap, StatusCode};

use crate::license::ControlPlaneValidator;
use crate::{config, store};

/// Process-global control-plane validator. `Some` when both `CONTROL_PLANE_URL`
/// and `ALTKEY_AGENT_TOKEN` are set at first access; `None` otherwise (local-only).
static CONTROL_PLANE: OnceLock<Option<ControlPlaneValidator>> = OnceLock::new();

fn control_plane() -> &'static Option<ControlPlaneValidator> {
    CONTROL_PLANE.get_or_init(|| {
        match (config::control_plane_url(), config::agent_token()) {
            (Some(url), Some(tok)) => Some(ControlPlaneValidator::new(url, tok)),
            _ => None,
        }
    })
}

/// Control-plane gate applied to an already-locally-validated key.
///
/// Returns `Ok(())` when the control plane is unconfigured (default path) OR when
/// the configured control plane approves the key. Returns `Err` (401) only when
/// the control plane is configured and rejects the key. Call this AFTER the local
/// key check passes, from an async request handler.
pub async fn control_plane_ok(key: &str) -> Result<(), (StatusCode, &'static str)> {
    match control_plane() {
        None => Ok(()), // unconfigured → local-store behavior is authoritative
        Some(validator) => {
            if validator.validate(key).await {
                Ok(())
            } else {
                Err((StatusCode::UNAUTHORIZED, "key not authorized by control plane"))
            }
        }
    }
}

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
