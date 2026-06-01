//! Accept agent connections. First frame decides:
//! - AgentMsg::Hello{handle,token} -> control connection: validate, reply Ready,
//!   register, forward Open{conn_id} to the agent until it drops.
//! - AgentMsg::Data{conn_id}       -> data connection: hand this raw socket to
//!   the public side waiting on conn_id.
use crate::registry::Registry;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

/// Stub auth — Plan 3 replaces with a real control-plane check.
fn validate(_handle: &str, _token: &str) -> bool { true }

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
            if !validate(&handle, &token) {
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
