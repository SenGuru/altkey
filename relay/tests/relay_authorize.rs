//! Integration test: the relay's `validate()` calls the control plane's
//! `/internal/agent/authorize` and honors its verdict.
//!
//! Rather than boot a full control-plane, we stand up a tiny axum stub server on
//! an ephemeral port that returns `ok:true` only when the body's `agent_token`
//! equals "good-token" AND the `x-altkey-service-secret` header equals "svc".
//! Then we drive `agent_conn::handle_for_test` with a Hello and assert the relay
//! registers the handle for a good token and rejects a bad one.
//!
//! Env-var leakage note: each `tests/*.rs` file is compiled into its OWN test
//! binary and run in a SEPARATE process by cargo, so `CONTROL_PLANE_URL` set here
//! does not leak into `agent_register`/`tunnel_e2e`/`pending_reclaim` (which run
//! in their own processes and expect accept-when-unset). Within this file both
//! sub-cases share the same process env, which is intended.

use altkey_api::dto::{AuthorizeRequest, AuthorizeResponse, Limits};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use std::time::Duration;
use tokio::net::TcpStream;
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

async fn authorize_stub(headers: HeaderMap, Json(req): Json<AuthorizeRequest>) -> Json<AuthorizeResponse> {
    let secret_ok = headers
        .get("x-altkey-service-secret")
        .and_then(|v| v.to_str().ok())
        .map(|s| s == "svc")
        .unwrap_or(false);
    let ok = secret_ok && req.agent_token == "good-token";
    Json(AuthorizeResponse {
        ok,
        account_id: if ok { "acct-1".into() } else { String::new() },
        plan: if ok { "pro".into() } else { String::new() },
        limits: Limits { max_concurrency: 0, max_rps: 0 },
    })
}

/// Boot the stub control plane on an ephemeral port; returns its base URL.
async fn spawn_stub() -> String {
    let app = Router::new().route("/internal/agent/authorize", post(authorize_stub));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Run a single Hello against the relay and return whether the handle registered.
async fn drive_hello(handle: &str, token: &str) -> (RelayMsg, bool) {
    let reg = altkey_relay::registry::Registry::new();
    let agent_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let agent_addr = agent_listener.local_addr().unwrap();
    {
        let reg = reg.clone();
        tokio::spawn(async move {
            let (sock, _) = agent_listener.accept().await.unwrap();
            altkey_relay::agent_conn::handle_for_test(reg, sock).await.ok();
        });
    }
    let mut c = TcpStream::connect(agent_addr).await.unwrap();
    write_msg(&mut c, &AgentMsg::Hello { handle: handle.into(), token: token.into() })
        .await
        .unwrap();
    let reply: RelayMsg = read_msg(&mut c).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let registered = reg.control_for(handle).is_some();
    (reply, registered)
}

// NOTE: All assertions live in a SINGLE #[tokio::test] so the process-global
// `CONTROL_PLANE_URL`/`INTERNAL_SERVICE_SECRET` env vars are mutated serially.
// Splitting into multiple test fns would let cargo's intra-binary parallelism
// race the env mutations (one case's URL leaking into another).
#[tokio::test]
async fn relay_calls_authorize_and_honors_verdict() {
    let base = spawn_stub().await;
    // Process-global env: set the control plane URL + service secret for this test process.
    std::env::set_var("CONTROL_PLANE_URL", &base);
    std::env::set_var("INTERNAL_SERVICE_SECRET", "svc");

    // Good token: control plane returns ok=true -> relay replies Ready and registers.
    let (reply, registered) = drive_hello("good-handle", "good-token").await;
    assert_eq!(reply, RelayMsg::Ready, "good token should be accepted");
    assert!(registered, "good token should register the handle");

    // Bad token: control plane returns ok=false -> relay rejects, no registration.
    let (reply, registered) = drive_hello("bad-handle", "bad-token").await;
    assert!(
        matches!(reply, RelayMsg::Reject { .. }),
        "bad token should be rejected, got {reply:?}"
    );
    assert!(!registered, "bad token must NOT register the handle");

    // Fail-closed: point CONTROL_PLANE_URL at a dead port; validate() must reject.
    // Bind then immediately drop the listener to obtain a port nothing listens on.
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);
    std::env::set_var("CONTROL_PLANE_URL", format!("http://{dead_addr}"));

    let (reply, registered) = drive_hello("unreachable-handle", "good-token").await;
    assert!(
        matches!(reply, RelayMsg::Reject { .. }),
        "unreachable control plane must fail closed, got {reply:?}"
    );
    assert!(!registered, "unreachable control plane must NOT register");
}
