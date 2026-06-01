//! altkey — Rust engine. Phase 1 port of the Python reference (Phase 0 main branch).
//! Same wire shape, same endpoints, same SQLite file. Two providers: Claude (OAuth)
//! and ChatGPT (Codex OAuth). Gemini intentionally parked.
mod config;
mod store;
mod auth;
mod license;
mod sse;
mod translate;
mod providers;
mod routes;
mod transparent;
mod tunnel_cert;
mod tunnel;

use anyhow::Result;
use std::net::SocketAddr;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("altkey=info,tower_http=info")))
        .init();

    store::init()?;

    // Best-effort claude_oauth refresh loop in the background.
    tokio::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60 * 30)).await;
            let _ = providers::claude_oauth::refresh_if_due().await;
        }
    });

    let app = routes::build_router();

    if std::env::var("ALTKEY_TRANSPARENT").as_deref() == Ok("1") {
        let app_for_transparent = routes::build_router();
        match transparent::enable(app_for_transparent).await {
            Ok(()) => tracing::info!("transparent mode ON (api.openai.com/api.anthropic.com intercepted)"),
            Err(e) => tracing::warn!("transparent mode failed: {e} (need admin/root?)"),
        }
    }

    if std::env::var("ALTKEY_TUNNEL").as_deref() == Ok("1") {
        let app = routes::build_router();
        let relay = config::relay_addr();
        let handle = config::handle();
        tokio::spawn(async move {
            if let Err(e) = tunnel::run(app, relay, handle).await {
                tracing::warn!("tunnel exited: {e}");
            }
        });
    }

    let addr: SocketAddr = "127.0.0.1:8787".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("altkey engine listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
