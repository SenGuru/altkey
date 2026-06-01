//! Public listener. For each inbound connection: peek the ClientHello SNI
//! (no decryption), map SNI host (<handle>.altkey.app) -> agent, ask the agent
//! to open a data connection, then splice the public socket <-> data socket.
//! The relay never decrypts; it forwards the original ClientHello bytes first.
use crate::registry::Registry;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tunnel_proto::sni::peek_sni;

/// Extract the handle from an SNI like "sen.altkey.app" -> "sen".
fn handle_from_sni(sni: &str) -> Option<&str> {
    sni.strip_suffix(".altkey.app").filter(|h| !h.is_empty())
}

pub async fn serve(reg: Registry, addr: String) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("public listener on {addr}");
    serve_listener(reg, listener).await
}

pub async fn serve_listener(reg: Registry, listener: TcpListener) -> anyhow::Result<()> {
    loop {
        let (sock, _) = listener.accept().await?;
        let reg = reg.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_public(reg, sock).await {
                tracing::debug!("public conn ended: {e}");
            }
        });
    }
}

pub(crate) async fn handle_public(reg: Registry, public: TcpStream) -> anyhow::Result<()> {
    handle_public_with_timeout(reg, public, Duration::from_secs(10)).await
}

/// Body of [`handle_public`] with an injectable data-wait timeout so tests can
/// exercise the timeout/reclaim path without a real 10s wait. The public entry
/// point keeps the 10s default.
#[cfg(not(any(test, feature = "test-helpers")))]
pub(crate) async fn handle_public_with_timeout(
    reg: Registry,
    public: TcpStream,
    wait: Duration,
) -> anyhow::Result<()> {
    handle_public_inner(reg, public, wait).await
}

#[cfg(any(test, feature = "test-helpers"))]
pub async fn handle_public_with_timeout(
    reg: Registry,
    public: TcpStream,
    wait: Duration,
) -> anyhow::Result<()> {
    handle_public_inner(reg, public, wait).await
}

async fn handle_public_inner(
    reg: Registry,
    mut public: TcpStream,
    wait: Duration,
) -> anyhow::Result<()> {
    let (sni, buffered) = peek_sni(&mut public).await?;
    let handle = handle_from_sni(&sni)
        .ok_or_else(|| anyhow::anyhow!("unknown sni {sni}"))?
        .to_string();
    let control = reg
        .control_for(&handle)
        .ok_or_else(|| anyhow::anyhow!("no agent for handle {handle}"))?;

    let (conn_id, data_rx) = reg.reserve_conn();
    // From here on the registry holds a pending sender for `conn_id`. On the happy
    // path the agent's data conn consumes it via `take_pending`. On any failure
    // path below we must reclaim it ourselves, or the entry leaks forever (a slow
    // unbounded-memory DoS via agents that never dial back).
    if control.send(conn_id).await.is_err() {
        reg.take_pending(conn_id);
        anyhow::bail!("agent control closed");
    }

    let mut data = match tokio::time::timeout(wait, data_rx).await {
        Ok(Ok(data)) => data,
        // Timeout, or the sender was dropped without delivering a socket. The
        // pending entry (if still present) is now stale — reclaim it. A benign
        // race where the agent delivers just after the timeout is harmless:
        // `take_pending` simply returns `None`.
        Ok(Err(_)) => {
            reg.take_pending(conn_id);
            anyhow::bail!("agent data conn dropped");
        }
        Err(_) => {
            reg.take_pending(conn_id);
            anyhow::bail!("timeout waiting for agent data conn");
        }
    };

    // Replay the buffered ClientHello to the agent, then splice raw bytes both ways.
    data.write_all(&buffered).await?;
    data.flush().await?;
    tokio::io::copy_bidirectional(&mut public, &mut data).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_extracted_from_sni() {
        assert_eq!(handle_from_sni("sen.altkey.app"), Some("sen"));
        assert_eq!(handle_from_sni("altkey.app"), None);
        assert_eq!(handle_from_sni(".altkey.app"), None);
        assert_eq!(handle_from_sni("foo.example.com"), None);
    }
}
