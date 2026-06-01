//! The three standard providers (Google, Microsoft, GitHub) + the oauth2-crate
//! plumbing for authorize URL and code→token→userinfo exchange.
//! Email/id are read from each provider's userinfo JSON.
//!
//! oauth2 5.x API used:
//!   - BasicClient::new(client_id)  — single arg, builder pattern
//!   - .set_client_secret(ClientSecret::new(...))
//!   - .set_auth_uri(AuthUrl::new(...)?)
//!   - .set_token_uri(TokenUrl::new(...)?)
//!   - .set_redirect_uri(RedirectUrl::new(...)?)
//!   - .authorize_url(CsrfToken::new_random)
//!   - .exchange_code(...).set_pkce_verifier(...).request_async(&http_client)
//!     where http_client is reqwest::Client built with redirect::Policy::none()

use super::Provider;
use anyhow::{anyhow, Result};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
};

fn redirect_uri(provider: &str) -> String {
    // PUBLIC_BASE_URL is read at call time so dev + prod differ without code change.
    let base =
        std::env::var("PUBLIC_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    format!("{base}/auth/{provider}/callback")
}

/// Returns (authorize_url_string, csrf_state_secret, pkce_verifier_secret).
pub fn authorize_url(p: &Provider) -> (String, String, String) {
    // In oauth2 5.x BasicClient::new takes only client_id; each endpoint is set via
    // a dedicated setter that transitions the typestate. The fully-built client (with
    // both auth_uri and token_uri set) supports both authorize_url and exchange_code.
    let client = BasicClient::new(ClientId::new(p.client_id.clone()))
        .set_client_secret(ClientSecret::new(p.client_secret.clone()))
        .set_auth_uri(
            AuthUrl::new(p.auth_url.clone()).expect("valid auth_url"),
        )
        .set_token_uri(
            TokenUrl::new(p.token_url.clone()).expect("valid token_url"),
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri(&p.name)).expect("valid redirect_uri"),
        );

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let mut req = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge);
    for s in &p.scopes {
        req = req.add_scope(Scope::new(s.clone()));
    }
    let (url, csrf) = req.url();
    (
        url.to_string(),
        csrf.secret().clone(),
        pkce_verifier.secret().clone(),
    )
}

/// Exchange the authorization code (with PKCE verifier) for a token, then fetch
/// the provider's userinfo endpoint and return (provider_user_id, email).
pub async fn exchange_and_fetch_userinfo(
    p: &Provider,
    code: &str,
    verifier: &str,
) -> Result<(String, String)> {
    // Build the OAuth2 client with both auth and token endpoints set (required for
    // exchange_code which needs EndpointSet on HasTokenUrl).
    let client = BasicClient::new(ClientId::new(p.client_id.clone()))
        .set_client_secret(ClientSecret::new(p.client_secret.clone()))
        .set_auth_uri(AuthUrl::new(p.auth_url.clone())?)
        .set_token_uri(TokenUrl::new(p.token_url.clone())?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri(&p.name))?);

    // Build a reqwest client that does NOT follow redirects (SSRF protection per
    // the oauth2 crate's security warning).
    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| anyhow!("failed to build HTTP client: {e}"))?;

    // request_async accepts any AsyncHttpClient; reqwest::Client implements it when
    // the oauth2 "reqwest" feature is enabled.
    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_string()))
        .request_async(&http_client)
        .await
        .map_err(|e| anyhow!("token exchange failed: {e}"))?;

    let access_token = token_response.access_token().secret().clone();

    // Fetch userinfo using a plain reqwest client.
    let userinfo_client = reqwest::Client::new();
    let json: serde_json::Value = userinfo_client
        .get(&p.userinfo_url)
        .bearer_auth(&access_token)
        .header("User-Agent", "altkey-control-plane")
        .send()
        .await?
        .json()
        .await?;

    let uid = json
        .get(&p.id_field)
        .map(|v| v.to_string().trim_matches('"').to_string())
        .ok_or_else(|| anyhow!("no {} in userinfo", p.id_field))?;
    let email = json
        .get(&p.email_field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("no {} in userinfo", p.email_field))?
        .to_string();
    Ok((uid, email))
}

/// Build Google / Microsoft / GitHub providers from env vars.
/// A provider is included only when both `{UPPER}_CLIENT_ID` and
/// `{UPPER}_CLIENT_SECRET` environment variables are set.
pub fn from_env() -> Vec<Provider> {
    let mut out = Vec::new();

    let mk = |name: &str,
              auth: &str,
              token: &str,
              userinfo: &str,
              scopes: &[&str],
              id_field: &str,
              email_field: &str|
     -> Option<Provider> {
        let up = name.to_uppercase();
        let client_id = std::env::var(format!("{up}_CLIENT_ID")).ok()?;
        let client_secret = std::env::var(format!("{up}_CLIENT_SECRET")).ok()?;
        Some(Provider {
            name: name.to_string(),
            client_id,
            client_secret,
            auth_url: auth.to_string(),
            token_url: token.to_string(),
            userinfo_url: userinfo.to_string(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            id_field: id_field.to_string(),
            email_field: email_field.to_string(),
        })
    };

    if let Some(p) = mk(
        "google",
        "https://accounts.google.com/o/oauth2/v2/auth",
        "https://oauth2.googleapis.com/token",
        "https://openidconnect.googleapis.com/v1/userinfo",
        &["openid", "email", "profile"],
        "sub",
        "email",
    ) {
        out.push(p);
    }
    if let Some(p) = mk(
        "microsoft",
        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        "https://graph.microsoft.com/oidc/userinfo",
        &["openid", "email", "profile"],
        "sub",
        "email",
    ) {
        out.push(p);
    }
    if let Some(p) = mk(
        "github",
        "https://github.com/login/oauth/authorize",
        "https://github.com/login/oauth/access_token",
        "https://api.github.com/user",
        &["read:user", "user:email"],
        "id",
        "email",
    ) {
        out.push(p);
    }
    out
}
