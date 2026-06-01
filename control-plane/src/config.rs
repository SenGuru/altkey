//! Process configuration, all from the environment (12-factor). Dev defaults to a
//! local SQLite file so the server runs with zero setup; prod sets DATABASE_URL to
//! a Postgres URL. No secrets are hardcoded.
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Config {
    /// SeaORM connection URL. `sqlite://…` or `postgres://…`.
    pub database_url: String,
    /// Public base URL of this service (used to build OAuth redirect URIs later).
    pub public_base_url: String,
    /// Shared secret the relay presents on /internal/* calls (set in 3d; optional now).
    pub internal_service_secret: Option<String>,
    /// Address to bind the HTTP server.
    pub bind_addr: String,
    // --- Polar billing (3c) ---
    pub polar_access_token: Option<String>,
    pub polar_webhook_secret: Option<String>,
    pub polar_base_url: String,
    /// Polar product IDs per plan (so a webhook product maps back to our Plan).
    pub polar_product_founding: Option<String>,
    pub polar_product_standard: Option<String>,
    pub polar_product_pro: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Config> {
        Ok(Config {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://./control-plane.db?mode=rwc".into()),
            public_base_url: std::env::var("PUBLIC_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            internal_service_secret: std::env::var("INTERNAL_SERVICE_SECRET").ok(),
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into()),
            polar_access_token: std::env::var("POLAR_ACCESS_TOKEN").ok(),
            polar_webhook_secret: std::env::var("POLAR_WEBHOOK_SECRET").ok(),
            polar_base_url: std::env::var("POLAR_BASE_URL")
                .unwrap_or_else(|_| "https://api.polar.sh".into()),
            polar_product_founding: std::env::var("POLAR_PRODUCT_FOUNDING").ok(),
            polar_product_standard: std::env::var("POLAR_PRODUCT_STANDARD").ok(),
            polar_product_pro: std::env::var("POLAR_PRODUCT_PRO").ok(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane_when_env_absent() {
        // Clear the vars this test cares about so defaults apply deterministically.
        for k in [
            "DATABASE_URL", "PUBLIC_BASE_URL", "BIND_ADDR", "INTERNAL_SERVICE_SECRET",
            "POLAR_ACCESS_TOKEN", "POLAR_WEBHOOK_SECRET", "POLAR_BASE_URL",
            "POLAR_PRODUCT_FOUNDING", "POLAR_PRODUCT_STANDARD", "POLAR_PRODUCT_PRO",
        ] {
            std::env::remove_var(k);
        }
        let c = Config::from_env().unwrap();
        assert!(c.database_url.starts_with("sqlite://"));
        assert_eq!(c.bind_addr, "127.0.0.1:8080");
        assert!(c.internal_service_secret.is_none());
    }
}
