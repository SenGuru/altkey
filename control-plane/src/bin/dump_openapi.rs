//! Print the control-plane OpenAPI document (including all router-merged paths) to
//! stdout. The web build pipes this into web/openapi.json for client codegen.
//!
//! Usage:
//!   cargo run -p control-plane --bin dump_openapi > web/openapi.json
use control_plane::state::AppState;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Build the AppState with an in-memory SQLite (NOT migrated — we only want the spec).
    let db = sea_orm::Database::connect("sqlite::memory:").await.expect("db");
    let config = control_plane::config::Config::from_env().expect("config");
    let state = AppState {
        db,
        config,
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: Arc::new(control_plane::billing::polar::FakePolarClient),
    };
    let spec = control_plane::app::openapi_json(state);
    println!("{spec}");
}
