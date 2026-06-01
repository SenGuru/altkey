//! Accept agent connections. First frame decides:
//! - AgentMsg::Hello{handle,token} -> control connection: validate, reply Ready,
//!   register, forward Open{conn_id} to the agent until it drops.
//! - AgentMsg::Data{conn_id}       -> data connection: hand this raw socket to
//!   the public side waiting on conn_id.
use crate::registry::Registry;
use altkey_api::dto::{AuthorizeRequest, AuthorizeResponse};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

/// Validate a handle+agent_token at tunnel connect. If `CONTROL_PLANE_URL` is unset
/// (dev/test), accept (the control plane is the prod authority; tests run without it).
/// When configured, call the control plane's `/internal/agent/authorize` with the
/// service secret and honor its verdict; fail closed on a network error.
async fn validate(handle: &str, token: &str) -> bool {
    let Ok(base) = std::env::var("CONTROL_PLANE_URL") else { return true; };
    let secret = std::env::var("INTERNAL_SERVICE_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/internal/agent/authorize"))
        .header("x-altkey-service-secret", secret)
        .json(&AuthorizeRequest { handle: handle.to_string(), agent_token: token.to_string() })
        .send()
        .await;
    match resp {
        Ok(r) => r.json::<AuthorizeResponse>().await.map(|a| a.ok).unwrap_or(false),
        Err(e) => {
            tracing::warn!("authorize call failed: {e}");
            false // fail closed when configured-but-unreachable
        }
    }
}

pub async fn serve(reg: Registry, addr: String) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("agent listener on {addr}");
    serve_listener(reg, listener).await
}

pub async fn serve_listener(reg: Registry, listener: TcpListener) -> anyhow::Result<()> {
    loop {
        let (sock, _) = listener.accept().await?;
        let reg = reg.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(reg, sock).await {
                tracing::debug!("agent conn ended: {e}");
            }
        });
    }
}

pub(crate) async fn handle(reg: Registry, mut sock: TcpStream) -> anyhow::Result<()> {
    let first: AgentMsg = read_msg(&mut sock).await?;
    match first {
        AgentMsg::Hello { handle, token } => {
            if !validate(&handle, &token).await {
                write_msg(&mut sock, &RelayMsg::Reject { reason: "invalid".into() }).await?;
                return Ok(());
            }
            write_msg(&mut sock, &RelayMsg::Ready).await?;
            let (tx, mut rx) = mpsc::channel::<u64>(64);
            reg.register_handle(handle.clone(), tx);
            tracing::info!("agent registered handle {handle}");
            let res = async {
                while let Some(conn_id) = rx.recv().await {
                    write_msg(&mut sock, &RelayMsg::Open { conn_id }).await?;
                }
                Ok::<(), anyhow::Error>(())
            }.await;
            reg.unregister_handle(&handle);
            res
        }
        AgentMsg::Data { conn_id } => {
            if let Some(slot) = reg.take_pending(conn_id) {
                let _ = slot.send(sock);
            }
            Ok(())
        }
    }
}

#[cfg(any(test, feature = "test-helpers"))]
pub async fn handle_for_test(reg: Registry, sock: TcpStream) -> anyhow::Result<()> {
    handle(reg, sock).await
}
