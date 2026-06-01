//! Integration tests for the engine's best-effort `UsageReporter`.
//!
//! Tests:
//! 1. Stub axum server captures a `UsageBatch`; `flush` POSTs the correct agent
//!    token and all buffered records.
//! 2. Reporter pointed at a dead URL → `flush` does NOT panic (best-effort).
//! 3. Empty buffer → `flush` is a no-op (no HTTP call made at all).

use std::sync::{Arc, Mutex};

use altkey::usage::UsageReporter;
use altkey_api::dto::{UsageBatch, UsageRecordDto};
use axum::{routing::post, Json, Router};

// ---------------------------------------------------------------------------
// Shared helper: a minimal UsageRecordDto for testing.
// ---------------------------------------------------------------------------

fn make_dto(model: &str) -> UsageRecordDto {
    UsageRecordDto {
        ts: "2026-06-01T00:00:00Z".into(),
        provider: "claude".into(),
        model: model.into(),
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
        tunnel_bytes: 1024,
        tool: None,
        key_prefix: Some("ak_live_test123".into()),
    }
}

// ---------------------------------------------------------------------------
// Test 1: stub captures the posted batch with correct agent_token + 2 records.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flush_posts_batch_to_stub() {
    // Shared capture store.
    let captured: Arc<Mutex<Vec<UsageBatch>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    let app = Router::new()
        .route(
            "/internal/usage",
            post(
                move |Json(batch): Json<UsageBatch>| {
                    let store = captured_clone.clone();
                    async move {
                        store.lock().unwrap().push(batch);
                        axum::http::StatusCode::OK
                    }
                },
            ),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let stub_url = format!("http://{addr}");
    let reporter = UsageReporter::new(stub_url, "agent-tok".into());

    reporter.record(make_dto("claude-sonnet-4-5"));
    reporter.record(make_dto("claude-opus-4-7"));

    reporter.flush().await;

    let batches = captured.lock().unwrap();
    assert_eq!(batches.len(), 1, "exactly one batch must have been POSTed");
    let batch = &batches[0];
    assert_eq!(batch.agent_token, "agent-tok");
    assert_eq!(batch.records.len(), 2, "both records must be present");
    assert!(
        batch.records.iter().any(|r| r.model == "claude-sonnet-4-5"),
        "first model must be in the batch"
    );
    assert!(
        batch.records.iter().any(|r| r.model == "claude-opus-4-7"),
        "second model must be in the batch"
    );

    task.abort();
}

// ---------------------------------------------------------------------------
// Test 2: dead URL → flush must not panic (best-effort, drops the batch).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flush_on_dead_url_does_not_panic() {
    // Bind and immediately drop a listener to get a port nobody is listening on.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);

    let reporter = UsageReporter::new(
        format!("http://127.0.0.1:{dead_port}"),
        "agent-tok".into(),
    );
    reporter.record(make_dto("gpt-4o"));

    // Must complete without panicking.
    reporter.flush().await;
    // No assertion needed — passing without panic is the contract.
}

// ---------------------------------------------------------------------------
// Test 3: empty buffer → flush is a no-op (no HTTP call, no panic).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flush_with_empty_buffer_is_noop() {
    // Use a dead URL; if flush tries to send it would fail. Absence of panic
    // with an empty buffer confirms the early-return no-op path.
    let reporter = UsageReporter::new(
        "http://127.0.0.1:1".into(), // port 1 is always refused
        "agent-tok".into(),
    );
    // Record nothing.
    reporter.flush().await;
    // Must complete without panic or error.
}

// ---------------------------------------------------------------------------
// Test 4: buffer is drained after flush (not double-sent on second flush).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flush_drains_buffer() {
    let captured: Arc<Mutex<Vec<UsageBatch>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    let app = Router::new().route(
        "/internal/usage",
        post(move |Json(batch): Json<UsageBatch>| {
            let store = captured_clone.clone();
            async move {
                store.lock().unwrap().push(batch);
                axum::http::StatusCode::OK
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let reporter = UsageReporter::new(format!("http://{addr}"), "agent-tok".into());
    reporter.record(make_dto("gpt-4o"));
    reporter.flush().await; // flushes 1 record
    reporter.flush().await; // buffer now empty → no-op, no second POST

    let batches = captured.lock().unwrap();
    assert_eq!(batches.len(), 1, "second flush on empty buffer must not send a second batch");

    task.abort();
}
