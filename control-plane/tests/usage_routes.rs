//! Integration tests for GET /usage/summary and GET /usage/records.
//! Boots the full app, seeds two accounts + sessions, ingests records via store,
//! and asserts:
//!   - GET /usage/summary returns rollups with correct totals for the session's account.
//!   - GET /usage/records returns raw records newest-first.
//!   - /usage/summary appears in the served OpenAPI spec.
//!   - A different account's session sees zero rows (isolation).
use altkey_api::dto::UsageRecordDto;
use chrono::{Duration, Utc};
use control_plane::{app, auth::session, config::Config, entities::account, state::AppState};
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};
use std::sync::Arc;
use uuid::Uuid;

async fn boot() -> (String, sea_orm::DatabaseConnection) {
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
    let router = app::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{addr}"), db)
}

async fn seed_account(db: &sea_orm::DatabaseConnection, email: &str) -> (Uuid, String) {
    let id = Uuid::new_v4();
    account::ActiveModel {
        id: Set(id),
        email: Set(email.into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
    }
    .insert(db)
    .await
    .unwrap();
    let token = session::issue(db, id).await.unwrap();
    (id, token)
}

fn dto(ts_offset_days: i64, model: &str, total_tokens: i64, bytes: i64) -> UsageRecordDto {
    let ts = (Utc::now() - Duration::days(ts_offset_days)).to_rfc3339();
    UsageRecordDto {
        ts,
        provider: "openai".into(),
        model: model.to_string(),
        prompt_tokens: total_tokens / 2,
        completion_tokens: total_tokens / 2,
        total_tokens,
        tunnel_bytes: bytes,
        tool: Some("chat".into()),
        key_prefix: None,
    }
}

#[tokio::test]
async fn usage_summary_and_records_work_with_isolation() {
    let (base, db) = boot().await;
    let client = reqwest::Client::new();

    // Seed two accounts
    let (acct1_id, sess1) = seed_account(&db, "user1@example.com").await;
    let (acct2_id, sess2) = seed_account(&db, "user2@example.com").await;

    let cookie1 = format!("altkey_session={sess1}");
    let cookie2 = format!("altkey_session={sess2}");

    // Ingest records for account 1: 2 records today (gpt-4o), 1 record yesterday (gpt-4o)
    let records_acct1 = vec![
        dto(0, "gpt-4o", 200, 1024),
        dto(0, "gpt-4o", 200, 1024),
        dto(1, "gpt-4o", 300, 2048),
    ];
    control_plane::usage::store::insert_records(&db, acct1_id, None, &records_acct1)
        .await
        .unwrap();

    // Ingest records for account 2 (different values — must not bleed through)
    let records_acct2 = vec![dto(0, "claude-3", 999, 9999)];
    control_plane::usage::store::insert_records(&db, acct2_id, None, &records_acct2)
        .await
        .unwrap();

    // ── GET /usage/summary for account 1 ─────────────────────────────────────
    let r = client
        .get(format!("{base}/usage/summary"))
        .header("Cookie", &cookie1)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "summary should return 200; status={}", r.status());

    let summary: serde_json::Value = r.json().await.unwrap();
    let rows = summary.as_array().unwrap();
    // Should have 2 rollup rows: today + yesterday (both gpt-4o)
    assert_eq!(rows.len(), 2, "account 1 should have 2 rollup rows (today + yesterday)");

    // Total tokens across all rows for account 1: 200+200+300 = 700
    let total_tokens: i64 = rows
        .iter()
        .map(|r| r["sum_tokens"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(total_tokens, 700, "total tokens should be 700 for account 1");

    // Total requests: 3
    let total_requests: i64 = rows
        .iter()
        .map(|r| r["sum_requests"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(total_requests, 3, "total requests should be 3");

    // All rows must be scoped to gpt-4o, not claude-3
    for row in rows {
        assert_ne!(
            row["model"].as_str().unwrap_or(""),
            "claude-3",
            "account 1 should never see claude-3 rows"
        );
    }

    // ── GET /usage/records for account 1 ─────────────────────────────────────
    let r = client
        .get(format!("{base}/usage/records"))
        .header("Cookie", &cookie1)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let records: serde_json::Value = r.json().await.unwrap();
    let rec_arr = records.as_array().unwrap();
    assert_eq!(rec_arr.len(), 3, "account 1 should have 3 raw records");

    // Verify newest-first ordering (ts descending)
    // The ts values are RFC3339; string compare works for ISO dates
    let timestamps: Vec<String> = rec_arr
        .iter()
        .map(|r| r["ts"].as_str().unwrap_or("").to_string())
        .collect();
    let mut sorted_desc = timestamps.clone();
    sorted_desc.sort_by(|a, b| b.cmp(a));
    assert_eq!(timestamps, sorted_desc, "records should be newest-first");

    // ── Isolation: account 2 sees only its own rows ───────────────────────────
    let r = client
        .get(format!("{base}/usage/summary"))
        .header("Cookie", &cookie2)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let summary2: serde_json::Value = r.json().await.unwrap();
    let rows2 = summary2.as_array().unwrap();
    assert_eq!(rows2.len(), 1, "account 2 should have 1 rollup row");
    assert_eq!(
        rows2[0]["sum_tokens"].as_i64().unwrap(),
        999,
        "account 2 tokens should be 999 (its own records)"
    );

    let r = client
        .get(format!("{base}/usage/records"))
        .header("Cookie", &cookie2)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let recs2: serde_json::Value = r.json().await.unwrap();
    assert_eq!(
        recs2.as_array().unwrap().len(),
        1,
        "account 2 should see only its own 1 record"
    );

    // ── Unauthenticated → 401 ────────────────────────────────────────────────
    let r = client
        .get(format!("{base}/usage/summary"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401, "unauthenticated summary must return 401");

    let r = client
        .get(format!("{base}/usage/records"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401, "unauthenticated records must return 401");

    // ── /usage/summary appears in the OpenAPI spec ───────────────────────────
    let spec: serde_json::Value = client
        .get(format!("{base}/api-docs/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        spec["paths"]["/usage/summary"].is_object(),
        "/usage/summary must appear in the OpenAPI spec"
    );
    assert!(
        spec["paths"]["/usage/records"].is_object(),
        "/usage/records must appear in the OpenAPI spec"
    );
}

#[tokio::test]
async fn usage_summary_returns_empty_for_account_with_no_records() {
    let (base, db) = boot().await;
    let client = reqwest::Client::new();

    let (_id, sess) = seed_account(&db, "nodata@example.com").await;
    let cookie = format!("altkey_session={sess}");

    let r = client
        .get(format!("{base}/usage/summary"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let summary: serde_json::Value = r.json().await.unwrap();
    assert_eq!(
        summary.as_array().unwrap().len(),
        0,
        "no records → empty summary"
    );
}
