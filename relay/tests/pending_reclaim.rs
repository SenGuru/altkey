//! Regression tests for the pending-conn slot leak: when the agent never dials
//! back a data connection, `handle_public` must reclaim the reserved `pending`
//! entry on timeout instead of leaking it (a slow unbounded-memory DoS).
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Build a real TLS ClientHello for `host` so `peek_sni` succeeds. Mirrors the
/// technique in `tunnel-proto/src/sni.rs`'s test.
fn client_hello_for(host: &str) -> Vec<u8> {
    use std::sync::Arc;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = rustls::RootCertStore::empty();
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(root)
        .with_no_client_auth();
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string()).unwrap();
    let mut conn = rustls::ClientConnection::new(Arc::new(cfg), server_name).unwrap();
    let mut buf = Vec::new();
    while conn.wants_write() {
        if conn.write_tls(&mut buf).unwrap() == 0 {
            break;
        }
    }
    buf
}

/// Connected loopback TcpStream pair: returns (server_side, client_side).
async fn loopback_pair() -> (TcpStream, TcpStream) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let connect = tokio::spawn(async move { TcpStream::connect(addr).await.unwrap() });
    let (server, _) = listener.accept().await.unwrap();
    let client = connect.await.unwrap();
    (server, client)
}

/// (a) When the registered agent accepts the `Open` (its control channel stays
/// open) but never dials a data connection back, the data-wait timeout must fire
/// AND the reserved pending slot must be reclaimed. Before the fix this leaks one
/// entry per abandoned public connection (`pending_len() == 1`); after the fix it
/// is 0.
#[tokio::test]
async fn timeout_reclaims_pending_slot() {
    let reg = altkey_relay::registry::Registry::new();

    // Register a handle's control sender directly and hold the receiver, never
    // acting on the forwarded conn_id. This stands in for a dead/slow agent that
    // accepts the Open but never opens a data conn — isolating the leak fix
    // without a live agent.
    let (control_tx, _control_rx) = mpsc::channel::<u64>(64);
    reg.register_handle("deadagent".into(), control_tx);

    // The public socket the relay reads the ClientHello from. We feed a real
    // ClientHello for <handle>.altkey.app so peek_sni / SNI routing succeed.
    let (public_sock, mut client) = loopback_pair().await;
    let hello = client_hello_for("deadagent.altkey.app");
    client.write_all(&hello).await.unwrap();
    client.flush().await.unwrap();

    let res = altkey_relay::public::handle_public_with_timeout(
        reg.clone(),
        public_sock,
        Duration::from_millis(200),
    )
    .await;

    assert!(res.is_err(), "expected a timeout error from the abandoned dial-back");
    assert_eq!(
        reg.pending_len(),
        0,
        "pending slot must be reclaimed on timeout (before the fix this is 1 = leaked)"
    );
    // Keep the control receiver alive until after the call so the channel stays
    // open (forcing the timeout path rather than a control-closed error).
    drop(_control_rx);
}

/// (b) An unknown handle returns an error BEFORE ever reserving a pending slot,
/// so `pending_len()` stays 0. Guards against a regression that would reserve
/// before validating the handle.
#[tokio::test]
async fn unknown_handle_never_reserves() {
    let reg = altkey_relay::registry::Registry::new();

    let (public_sock, mut client) = loopback_pair().await;
    let hello = client_hello_for("nobody.altkey.app");
    client.write_all(&hello).await.unwrap();
    client.flush().await.unwrap();

    let res = altkey_relay::public::handle_public_with_timeout(
        reg.clone(),
        public_sock,
        Duration::from_millis(200),
    )
    .await;

    assert!(res.is_err(), "unknown handle should error");
    assert_eq!(reg.pending_len(), 0, "no pending slot should be reserved for an unknown handle");
}
