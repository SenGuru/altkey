//! Tests the engine's ControlPlaneValidator: positive/negative verdicts, the 60s
//! positive cache (a second call within TTL is served from cache, not re-fetched),
//! and fail-closed-from-cold when the control plane is unreachable with no prior
//! success.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use altkey::license::ControlPlaneValidator;
use altkey_api::dto::{KeyValidateRequest, KeyValidateResponse};
use axum::{extract::State, routing::post, Json, Router};

/// Stub control plane: returns valid+sub_active for "ak_live_good", invalid
/// otherwise, and counts how many validate calls it actually received.
async fn spawn_stub() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/internal/key/validate", post(validate_handler))
        .with_state(hits.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), hits, task)
}

async fn validate_handler(
    State(hits): State<Arc<AtomicUsize>>,
    Json(req): Json<KeyValidateRequest>,
) -> Json<KeyValidateResponse> {
    hits.fetch_add(1, Ordering::SeqCst);
    if req.key == "ak_live_good" {
        Json(KeyValidateResponse {
            valid: true,
            sub_active: true,
            plan: "standard".into(),
        })
    } else {
        Json(KeyValidateResponse {
            valid: false,
            sub_active: false,
            plan: String::new(),
        })
    }
}

#[tokio::test]
async fn valid_and_invalid_keys() {
    let (url, _hits, task) = spawn_stub().await;
    let v = ControlPlaneValidator::new(url, "agent-tok".into());
    assert!(v.validate("ak_live_good").await, "good key must be accepted");
    assert!(!v.validate("ak_live_bad").await, "bad key must be rejected");
    task.abort();
}

#[tokio::test]
async fn positive_cache_avoids_refetch_within_ttl() {
    let (url, hits, task) = spawn_stub().await;
    let v = ControlPlaneValidator::new(url, "agent-tok".into());

    assert!(v.validate("ak_live_good").await);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "first call hits the stub");

    // Second call within the 60s TTL must be served from cache — no new request.
    assert!(v.validate("ak_live_good").await);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "second call within TTL must be cache-served, not re-fetched"
    );
    task.abort();
}

#[tokio::test]
async fn fail_closed_from_cold_when_unreachable() {
    // Point at a dead port with no prior success → fail closed (no grace to fall
    // back on). Bind+drop a listener to obtain a definitely-free port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);

    let v = ControlPlaneValidator::new(format!("http://127.0.0.1:{dead_port}"), "agent-tok".into());
    assert!(
        !v.validate("ak_live_good").await,
        "cold + unreachable must fail closed"
    );
}
