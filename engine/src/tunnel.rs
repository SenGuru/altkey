//! Agent tunnel client. Opens a control connection to the relay, claims the
//! handle, and for each Open{conn_id} dials a data connection, terminates the
//! public TLS on it with the handle cert, and serves the existing router.
use crate::tunnel_cert::{HandleCert, SelfSignedHandleCert};
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::rustls::{
    crypto::ring as ring_provider,
    pki_types::{CertificateDer, PrivateKeyDer},
    ServerConfig,
};
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt; // for Router::oneshot
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

/// True while the tunnel control connection is up and Ready was confirmed.
pub static TUNNEL_UP: AtomicBool = AtomicBool::new(false);

pub fn is_up() -> bool {
    TUNNEL_UP.load(Ordering::SeqCst)
}

/// Ensure the process-wide default rustls CryptoProvider is installed (ring).
/// Idempotent: a second call returns Err, which we ignore — same pattern as
/// `transparent::server::install_ring_provider`.
fn install_ring_provider() {
    let _ = ring_provider::default_provider().install_default();
}

fn acceptor_for(cert: &dyn HandleCert, host: &str) -> Result<TlsAcceptor> {
    let (cert_pem, key_pem) = cert.cert_for(host)?;
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes()).filter_map(|c| c.ok()).collect();
    let key: PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut key_pem.as_bytes())?.ok_or_else(|| anyhow!("no key"))?;
    install_ring_provider();
    let cfg = ServerConfig::builder_with_provider(Arc::new(ring_provider::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

/// Connect to `relay_addr`, claim `handle`, and for each `Open{conn_id}` spawn
/// a task that dials a data connection, terminates TLS with the handle cert,
/// and serves `app`.
///
/// Public interface used by Task 8 (e2e test) and Task 9 (main.rs wiring).
pub async fn run(app: axum::Router, relay_addr: String, handle: String) -> Result<()> {
    let host = format!("{handle}.altkey.app");
    let cert: Arc<dyn HandleCert> = Arc::new(SelfSignedHandleCert);
    let acceptor = acceptor_for(cert.as_ref(), &host)?;

    let mut control = TcpStream::connect(&relay_addr).await?;
    write_msg(&mut control, &AgentMsg::Hello { handle: handle.clone(), token: "stub".into() }).await?;
    match read_msg::<_, RelayMsg>(&mut control).await? {
        RelayMsg::Ready => {}
        RelayMsg::Reject { reason } => return Err(anyhow!("relay rejected: {reason}")),
        other => return Err(anyhow!("unexpected relay reply: {other:?}")),
    }
    TUNNEL_UP.store(true, Ordering::SeqCst);
    tracing::info!("tunnel up: https://{host}/ via {relay_addr}");

    loop {
        match read_msg::<_, RelayMsg>(&mut control).await {
            Ok(RelayMsg::Open { conn_id }) => {
                let relay_addr = relay_addr.clone();
                let acceptor = acceptor.clone();
                let app = app.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_one(relay_addr, conn_id, acceptor, app).await {
                        tracing::debug!("tunnel conn {conn_id} ended: {e}");
                    }
                });
            }
            Ok(_) => {}
            Err(e) => {
                TUNNEL_UP.store(false, Ordering::SeqCst);
                return Err(anyhow!("control connection closed: {e}"));
            }
        }
    }
}

async fn serve_one(
    relay_addr: String,
    conn_id: u64,
    acceptor: TlsAcceptor,
    app: axum::Router,
) -> Result<()> {
    let mut data = TcpStream::connect(&relay_addr).await?;
    write_msg(&mut data, &AgentMsg::Data { conn_id }).await?;
    let tls = acceptor.accept(data).await?;
    let svc = hyper::service::service_fn(move |req| {
        let app = app.clone();
        async move { app.oneshot(req).await }
    });
    hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection(hyper_util::rt::TokioIo::new(tls), svc)
        .await
        .map_err(|e| anyhow!("serve: {e}"))?;
    Ok(())
}
