//! HTTPS listener that terminates TLS for the intercepted hosts (SNI-selected
//! leaf certs) and serves the existing axum router.
//!
//! Crypto provider: **ring**. reqwest's `rustls-tls` pulls the ring-backed
//! rustls provider, and `tokio-rustls` is pinned to `default-features = false,
//! features = ["ring", "tls12", "logging"]` so aws-lc-rs is fully out of the
//! tree. A single rustls + single (ring) provider keeps `sign::any_supported_type`
//! and `default_provider().install_default()` consistent.
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::rustls::{
    crypto::ring as ring_provider,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
    ServerConfig,
};
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt; // for Router::oneshot

use crate::config;
use crate::transparent::ca::Ca;

/// SNI-driven cert resolver: mints (and caches) one leaf cert per hostname,
/// signed by the local altkey CA.
struct SniResolver {
    keys: parking_lot::Mutex<std::collections::HashMap<String, Arc<CertifiedKey>>>,
    ca: Arc<Ca>,
}

impl std::fmt::Debug for SniResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SniResolver").finish_non_exhaustive()
    }
}

impl SniResolver {
    fn certified_for(&self, host: &str) -> Option<Arc<CertifiedKey>> {
        if let Some(k) = self.keys.lock().get(host) {
            return Some(k.clone());
        }
        let leaf = self.ca.mint_leaf(host).ok()?;
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut leaf.cert_pem.as_bytes())
            .filter_map(|c| c.ok())
            .collect();
        if certs.is_empty() {
            return None;
        }
        let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut leaf.key_pem.as_bytes()).ok()??;
        let signing = ring_provider::sign::any_supported_type(&key).ok()?;
        let ck = Arc::new(CertifiedKey::new(certs, signing));
        self.keys.lock().insert(host.to_string(), ck.clone());
        Some(ck)
    }
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let host = hello.server_name()?;
        self.certified_for(host)
    }
}

/// Ensure the process-wide default rustls CryptoProvider is installed (ring).
/// Idempotent: a second call returns Err, which we ignore.
fn install_ring_provider() {
    let _ = ring_provider::default_provider().install_default();
}

/// Build a rustls `ServerConfig` whose certs come from the SNI resolver.
fn build_server_config(ca: Arc<Ca>) -> Result<ServerConfig> {
    install_ring_provider();
    let resolver = Arc::new(SniResolver {
        keys: parking_lot::Mutex::new(Default::default()),
        ca,
    });
    let mut cfg = ServerConfig::builder_with_provider(Arc::new(ring_provider::default_provider()))
        .with_safe_default_protocol_versions()
        .context("rustls protocol versions")?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(cfg)
}

/// Spawn the accept loop that terminates TLS and routes each request through
/// `app`. Shared by `serve` and `serve_for_test`.
fn spawn_accept_loop(listener: TcpListener, acceptor: TlsAcceptor, app: axum::Router) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let svc = hyper::service::service_fn(move |req| {
                    let app = app.clone();
                    async move { app.oneshot(req).await }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(hyper_util::rt::TokioIo::new(tls), svc)
                    .await;
            });
        }
    })
}

/// Bind `127.0.0.1:443`, terminate TLS via the SNI resolver, and serve `app`.
/// Binding `:443` requires admin/root.
pub async fn serve(app: axum::Router, ca: Arc<Ca>) -> Result<tokio::task::JoinHandle<()>> {
    let cfg = build_server_config(ca)?;
    let acceptor = TlsAcceptor::from(Arc::new(cfg));
    let listener = TcpListener::bind(("127.0.0.1", 443))
        .await
        .context("bind :443 (needs admin/root)")?;
    Ok(spawn_accept_loop(listener, acceptor, app))
}

/// A running test server: the bound port plus the accept-loop task handle.
pub struct TestServer {
    pub port: u16,
    pub task: tokio::task::JoinHandle<()>,
}

/// Like [`serve`], but binds the GIVEN port (use `0` for an ephemeral port) so
/// integration tests don't need `:443`/admin. Returns the actual bound port.
pub async fn serve_for_test(app: axum::Router, ca: Arc<Ca>, port: u16) -> Result<TestServer> {
    let cfg = build_server_config(ca)?;
    let acceptor = TlsAcceptor::from(Arc::new(cfg));
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .context("bind test port")?;
    let port = listener.local_addr()?.port();
    let task = spawn_accept_loop(listener, acceptor, app);
    Ok(TestServer { port, task })
}

/// Load (or create) the local CA from the configured paths, wrapped in `Arc`.
pub fn load_ca() -> Result<Arc<Ca>> {
    Ok(Arc::new(Ca::load_or_create(&config::ca_cert_path(), &config::ca_key_path())?))
}
