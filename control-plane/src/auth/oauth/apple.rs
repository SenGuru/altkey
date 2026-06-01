//! Apple Sign In — stub for Task 5. Task 6 replaces this with the ES256
//! client-secret JWT + id_token email extraction implementation.
use super::Provider;

/// Exchange the Apple auth code and extract (provider_user_id, email) from the id_token.
/// Stub: returns an error until Task 6 is implemented.
pub async fn exchange_and_extract(
    _p: &Provider,
    _code: &str,
    _verifier: &str,
) -> anyhow::Result<(String, String)> {
    anyhow::bail!("apple not yet implemented")
}

/// Build the Apple provider from env (APPLE_CLIENT_ID).
/// Stub: returns None until Task 6 is implemented.
pub fn from_env() -> Option<Provider> {
    None
}
