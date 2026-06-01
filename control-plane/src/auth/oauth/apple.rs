//! Apple Sign In differs from the standard providers in two ways:
//!  1. the OAuth "client secret" is a short-lived ES256 JWT we sign with a .p8 key
//!     (APPLE_TEAM_ID / APPLE_KEY_ID / APPLE_PRIVATE_KEY / APPLE_CLIENT_ID), and
//!  2. the user's email is a claim inside the returned `id_token` (a JWT), not a
//!     userinfo endpoint.
//!
//! NOTE (for Task 7's implementer): Apple's authorize step requires
//! `response_mode=form_post` when requesting the `email` scope, which means Apple
//! POSTs the callback (with `code` + `state` as form fields) rather than issuing a
//! GET redirect. The generic `callback` handler currently accepts GET only. Task 7
//! should also mount `POST /auth/apple/callback` with a form-body variant so that
//! Apple Sign In works end-to-end in production.
use super::Provider;
use anyhow::{anyhow, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ClientSecretClaims {
    iss: String, // team id
    iat: i64,
    exp: i64,
    aud: String, // https://appleid.apple.com
    sub: String, // client id (service id)
}

/// Build the ES256 client-secret JWT Apple requires in place of a static secret.
/// `private_key_pem` must be a PKCS#8 EC P-256 private key in PEM format
/// (the `.p8` file content from Apple Developer Portal).
pub fn client_secret_jwt(
    team_id: &str,
    key_id: &str,
    client_id: &str,
    private_key_pem: &str,
    now: i64,
) -> Result<String> {
    let claims = ClientSecretClaims {
        iss: team_id.to_string(),
        iat: now,
        exp: now + 3600,
        aud: "https://appleid.apple.com".into(),
        sub: client_id.to_string(),
    };
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(key_id.to_string());
    let key = EncodingKey::from_ec_pem(private_key_pem.as_bytes())
        .map_err(|e| anyhow!("apple .p8 parse: {e}"))?;
    Ok(encode(&header, &claims, &key)?)
}

#[derive(Deserialize)]
struct IdTokenClaims {
    sub: String,
    email: Option<String>,
}

/// Decode (WITHOUT verifying signature — we just received it over TLS from Apple's
/// token endpoint in direct response to our request) the id_token's sub + email.
/// (A hardened version verifies against Apple's JWKS; acceptable for v1 since the
/// token came straight from the token endpoint, not the browser.)
fn extract_id_token(id_token: &str) -> Result<(String, String)> {
    let mut parts = id_token.split('.');
    let _h = parts.next();
    let payload = parts.next().ok_or_else(|| anyhow!("malformed id_token"))?;
    let bytes = base64_url_decode(payload)?;
    let claims: IdTokenClaims = serde_json::from_slice(&bytes)?;
    let email = claims
        .email
        .ok_or_else(|| anyhow!("apple id_token has no email"))?;
    Ok((claims.sub, email))
}

fn base64_url_decode(s: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s)?)
}

/// Exchange the auth code at Apple's token endpoint (using the signed client secret)
/// and return (provider_user_id, email) from the id_token.
pub async fn exchange_and_extract(
    p: &Provider,
    code: &str,
    _verifier: &str,
) -> Result<(String, String)> {
    let team_id = std::env::var("APPLE_TEAM_ID")?;
    let key_id = std::env::var("APPLE_KEY_ID")?;
    let private_key = std::env::var("APPLE_PRIVATE_KEY")?; // PEM contents of the .p8
    let now = chrono::Utc::now().timestamp();
    let client_secret =
        client_secret_jwt(&team_id, &key_id, &p.client_id, &private_key, now)?;

    let redirect = {
        let base = std::env::var("PUBLIC_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080".into());
        format!("{base}/auth/apple/callback")
    };
    let http = reqwest::Client::new();
    let resp: serde_json::Value = http
        .post(&p.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", p.client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect.as_str()),
        ])
        .send()
        .await?
        .json()
        .await?;
    let id_token = resp
        .get("id_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("apple token response missing id_token: {resp}"))?;
    extract_id_token(id_token)
}

/// Apple provider config from env (APPLE_CLIENT_ID is the Services ID).
pub fn from_env() -> Option<Provider> {
    let client_id = std::env::var("APPLE_CLIENT_ID").ok()?;
    // The .p8 key + team/key IDs are read at exchange time (APPLE_TEAM_ID,
    // APPLE_KEY_ID, APPLE_PRIVATE_KEY). Only CLIENT_ID is needed to enable the button.
    Some(Provider {
        name: "apple".into(),
        client_id,
        client_secret: String::new(), // computed per-request (signed JWT)
        auth_url: "https://appleid.apple.com/auth/authorize".into(),
        token_url: "https://appleid.apple.com/auth/token".into(),
        userinfo_url: String::new(), // unused; email comes from id_token
        scopes: vec!["name".into(), "email".into()],
        id_field: "sub".into(),
        email_field: "email".into(),
    })
}
