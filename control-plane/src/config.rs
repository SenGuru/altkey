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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane_when_env_absent() {
        // Clear the vars this test cares about so defaults apply deterministically.
        for k in ["DATABASE_URL", "PUBLIC_BASE_URL", "BIND_ADDR", "INTERNAL_SERVICE_SECRET"] {
            std::env::remove_var(k);
        }
        let c = Config::from_env().unwrap();
        assert!(c.database_url.starts_with("sqlite://"));
        assert_eq!(c.bind_addr, "127.0.0.1:8080");
        assert!(c.internal_service_secret.is_none());
    }
}
