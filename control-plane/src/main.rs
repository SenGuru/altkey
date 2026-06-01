//! Binary entrypoint: load env config, connect the DB, run migrations on boot,
//! then serve the axum app.
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("control_plane=info,tower_http=info")),
        )
        .init();

    let config = control_plane::config::Config::from_env()?;
    let bind_addr = config.bind_addr.clone();
    let db = control_plane::db::connect(&config).await?;
    control_plane::db::run_migrations(&db).await?;

    let app = control_plane::app::build(control_plane::state::AppState {
        db,
        polar: control_plane::billing::polar::from_config(&config),
        config,
        email: std::sync::Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: std::sync::Arc::new(control_plane::auth::oauth::registry_from_env()),
    });
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("control-plane listening on {}", bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
