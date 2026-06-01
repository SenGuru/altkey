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

pub(crate) async fn handle_public(reg: Registry, mut public: TcpStream) -> anyhow::Result<()> {
    let (sni, buffered) = peek_sni(&mut public).await?;
    let handle = handle_from_sni(&sni)
        .ok_or_else(|| anyhow::anyhow!("unknown sni {sni}"))?
        .to_string();
    let control = reg
        .control_for(&handle)
        .ok_or_else(|| anyhow::anyhow!("no agent for handle {handle}"))?;

    let (conn_id, data_rx) = reg.reserve_conn();
    control.send(conn_id).await.map_err(|_| anyhow::anyhow!("agent control closed"))?;

    let mut data = tokio::time::timeout(Duration::from_secs(10), data_rx)
        .await
        .map_err(|_| anyhow::anyhow!("timeout waiting for agent data conn"))??;

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
