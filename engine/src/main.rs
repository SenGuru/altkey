//! altkey — Rust engine. Phase 1 port of the Python reference (Phase 0 main branch).
//! Same wire shape, same endpoints, same SQLite file. Two providers: Claude (OAuth)
//! and ChatGPT (Codex OAuth). Gemini intentionally parked.
mod config;
mod store;
mod auth;
mod sse;
mod translate;
mod providers;
mod routes;
mod transparent;

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
    let addr: SocketAddr = "127.0.0.1:8787".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("altkey engine listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
