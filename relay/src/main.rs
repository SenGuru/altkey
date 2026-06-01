//! altkey relay — public SNI-passthrough listener + agent control/data listener.
use altkey_relay::registry::Registry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("altkey_relay=info")),
    ).init();
    let public_addr = std::env::var("RELAY_PUBLIC_ADDR").unwrap_or_else(|_| "0.0.0.0:443".into());
    let agent_addr = std::env::var("RELAY_AGENT_ADDR").unwrap_or_else(|_| "0.0.0.0:7000".into());
    Registry::new().run(&public_addr, &agent_addr).await
}
