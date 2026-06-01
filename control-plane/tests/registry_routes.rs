//! Integration test for the registry routes (handles, agents, keys).
//! Boots the app, seeds an account + session, then exercises the full lifecycle:
//!   availability check (true) → claim handle → availability (false) →
//!   pair agent (assert ak_agent_ token returned once; GET shows only prefix) →
//!   mint key (assert ak_live_ secret returned once; GET hides it) →
//!   delete key → GET /keys shows it gone/revoked.
//! Also asserts that /handles is present in the served openapi.json.
use control_plane::app;
use control_plane::auth::session;
use control_plane::config::Config;
use control_plane::entities::account;
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};
use std::sync::Arc;

async fn boot() -> (String, sea_orm::DatabaseConnection, uuid::Uuid, String) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let config = Config {
        database_url: "sqlite::memory:".into(),
        public_base_url: "http://localhost".into(),
        internal_service_secret: None,
        bind_addr: "127.0.0.1:0".into(),
        polar_access_token: None,
        polar_webhook_secret: None,
        polar_base_url: "https://api.polar.sh".into(),
        polar_product_founding: None,
        polar_product_standard: Some("prod_standard".into()),
        polar_product_pro: None,
    };
    let state = AppState {
        db: db.clone(),
        config,
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: Arc::new(control_plane::billing::polar::FakePolarClient),
    };
    let id = uuid::Uuid::new_v4();
    account::ActiveModel {
        id: Set(id),
        email: Set("reg@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();
    let token = session::issue(&db, id).await.unwrap();
    let appx = app::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, appx).await.unwrap() });
    (format!("http://{addr}"), db, id, token)
}

#[tokio::test]
async fn registry_routes_lifecycle() {
    let (base, _db, _acct, session_token) = boot().await;
    let client = reqwest::Client::new();
    let cookie = format!("altkey_session={session_token}");

    // ── Handles ──────────────────────────────────────────────────────────────

    // Check availability: "test-handle" should be free
    let r = client
        .get(format!("{base}/handles/availability?name=test-handle"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "availability check failed: {}", r.status());
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["available"], true, "expected test-handle to be available");

    // Claim the handle
    let r = client
        .post(format!("{base}/handles"))
        .header("Cookie", &cookie)
        .json(&serde_json::json!({ "name": "test-handle" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "claim handle failed: {}", r.status());
    let handle_resp: serde_json::Value = r.json().await.unwrap();
    let handle_id = handle_resp["id"].as_str().unwrap().to_string();
    assert_eq!(handle_resp["name"], "test-handle");
    assert_eq!(handle_resp["status"], "active");

    // Availability should now be false
    let r = client
        .get(format!("{base}/handles/availability?name=test-handle"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["available"], false, "expected test-handle to be taken after claim");

    // List handles — should contain the one we just claimed
    let r = client
        .get(format!("{base}/handles"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let handles: serde_json::Value = r.json().await.unwrap();
    assert!(handles.as_array().unwrap().iter().any(|h| h["id"] == handle_id));

    // Invalid handle names return available=false (not a 400)
    let r = client
        .get(format!("{base}/handles/availability?name=UPPERCASE"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["available"], false, "uppercase handle name should be unavailable (invalid)");

    // ── Agents ───────────────────────────────────────────────────────────────

    // Pair an agent — must return an ak_agent_ token ONCE
    let r = client
        .post(format!("{base}/agents"))
        .header("Cookie", &cookie)
        .json(&serde_json::json!({ "handle_id": handle_id, "name": "laptop" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "pair agent failed: {}", r.status());
    let agent_resp: serde_json::Value = r.json().await.unwrap();
    let agent_id = agent_resp["id"].as_str().unwrap().to_string();
    let agent_token = agent_resp["token"].as_str().unwrap();
    assert!(
        agent_token.starts_with("ak_agent_"),
        "agent token must start with ak_agent_, got: {agent_token}"
    );

    // GET /agents must show only the prefix, NOT the token
    let r = client
        .get(format!("{base}/agents"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let agents: serde_json::Value = r.json().await.unwrap();
    let agent_row = agents
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == agent_id)
        .expect("agent must appear in list");
    // The response must have token_prefix but NOT a "token" field with the full secret
    let prefix = agent_row["token_prefix"].as_str().unwrap();
    assert!(
        agent_token.starts_with(prefix),
        "token_prefix must be the start of the full token"
    );
    assert!(
        agent_row.get("token").is_none() || agent_row["token"].is_null(),
        "full token must NOT appear in list view"
    );

    // ── Keys ─────────────────────────────────────────────────────────────────

    // Mint a key — must return an ak_live_ secret ONCE
    let r = client
        .post(format!("{base}/keys"))
        .header("Cookie", &cookie)
        .json(&serde_json::json!({ "name": "prod key", "agent_id": agent_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "mint key failed: {}", r.status());
    let key_resp: serde_json::Value = r.json().await.unwrap();
    let key_id = key_resp["id"].as_str().unwrap().to_string();
    let key_secret = key_resp["secret"].as_str().unwrap();
    assert!(
        key_secret.starts_with("ak_live_"),
        "key secret must start with ak_live_, got: {key_secret}"
    );

    // GET /keys must show only the prefix, NOT the secret
    let r = client
        .get(format!("{base}/keys"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let keys: serde_json::Value = r.json().await.unwrap();
    let key_row = keys
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == key_id)
        .expect("key must appear in list");
    let key_prefix = key_row["key_prefix"].as_str().unwrap();
    assert!(
        key_secret.starts_with(key_prefix),
        "key_prefix must be the start of the full secret"
    );
    assert!(
        key_row.get("secret").is_none() || key_row["secret"].is_null(),
        "full secret must NOT appear in list view"
    );
    assert!(
        key_row["revoked_at"].is_null(),
        "key should not be revoked yet"
    );

    // Delete (revoke) the key
    let r = client
        .delete(format!("{base}/keys/{key_id}"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "delete key failed: {}", r.status());

    // After revocation, GET /keys should show it with revoked_at set
    let r = client
        .get(format!("{base}/keys"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let keys: serde_json::Value = r.json().await.unwrap();
    let revoked_key = keys
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == key_id)
        .expect("key must still appear in list after revocation");
    assert!(
        !revoked_key["revoked_at"].is_null(),
        "revoked_at must be set after deletion"
    );

    // ── Ownership checks ─────────────────────────────────────────────────────

    // Attempting to delete a handle with a garbage UUID returns 404
    let r = client
        .delete(format!("{base}/handles/00000000-0000-0000-0000-000000000000"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);

    // Unauthenticated request returns 401
    let r = client
        .get(format!("{base}/handles"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // ── OpenAPI spec includes /handles ───────────────────────────────────────

    let doc: serde_json::Value = client
        .get(format!("{base}/api-docs/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        doc["paths"]["/handles"].is_object(),
        "/handles must appear in the OpenAPI spec"
    );
}
