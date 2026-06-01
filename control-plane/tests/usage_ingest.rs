//! Tests for POST /internal/usage.
//! Seeds an account + handle + agent; POSTs a batch with the agent token → 200 + records stored.
//! Bad agent token → 401.
//! Uses the same direct-State-injection pattern as billing_webhook.rs.
use altkey_api::dto::{UsageBatch, UsageRecordDto};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use control_plane::billing::polar::FakePolarClient;
use control_plane::config::Config;
use control_plane::entities::{prelude::UsageRecord, usage_record};
use control_plane::internal::routes::ingest_usage;
use control_plane::registry::store::{claim_handle, pair_agent};
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use uuid::Uuid;

async fn make_state() -> (AppState, sea_orm::DatabaseConnection) {
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
    let st = AppState {
        db: db.clone(),
        config,
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: Arc::new(FakePolarClient),
    };
    (st, db)
}

fn make_record(ts: &str) -> UsageRecordDto {
    UsageRecordDto {
        ts: ts.to_string(),
        provider: "openai".into(),
        model: "gpt-4o".into(),
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        tunnel_bytes: 1024,
        tool: Some("chat_completions".into()),
        key_prefix: Some("ak_live_abcd".into()),
    }
}

#[tokio::test]
async fn ingest_with_valid_agent_token_stores_records() {
    let (st, db) = make_state().await;

    // Seed account + handle + agent
    let account_id = Uuid::new_v4();
    control_plane::entities::account::ActiveModel {
        id: Set(account_id),
        email: Set("ingest@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let handle = claim_handle(&db, account_id, "ingest-handle").await.unwrap();
    let pa = pair_agent(&db, account_id, handle.id, "test-agent").await.unwrap();
    let agent_token = pa.token_plaintext.clone();

    // POST batch — two records
    let now = Utc::now().to_rfc3339();
    let batch = UsageBatch {
        agent_token: agent_token.clone(),
        records: vec![make_record(&now), make_record(&now)],
    };

    let status = ingest_usage(State(st.clone()), Json(batch)).await;
    assert_eq!(status, StatusCode::OK, "valid agent token must return 200");

    // Verify records were stored
    let count = UsageRecord::find()
        .filter(usage_record::Column::AccountId.eq(account_id))
        .all(&db)
        .await
        .unwrap()
        .len();
    assert_eq!(count, 2, "two records should be stored");
}

#[tokio::test]
async fn ingest_with_bad_agent_token_returns_401() {
    let (st, _db) = make_state().await;

    let batch = UsageBatch {
        agent_token: "ak_agent_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".into(),
        records: vec![make_record(&Utc::now().to_rfc3339())],
    };

    let status = ingest_usage(State(st), Json(batch)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "bad agent token must return 401");
}

#[tokio::test]
async fn ingest_empty_batch_with_valid_token_is_200() {
    let (st, db) = make_state().await;

    let account_id = Uuid::new_v4();
    control_plane::entities::account::ActiveModel {
        id: Set(account_id),
        email: Set("empty@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let handle = claim_handle(&db, account_id, "empty-handle").await.unwrap();
    let pa = pair_agent(&db, account_id, handle.id, "empty-agent").await.unwrap();

    let batch = UsageBatch {
        agent_token: pa.token_plaintext,
        records: vec![],
    };

    let status = ingest_usage(State(st), Json(batch)).await;
    assert_eq!(status, StatusCode::OK, "empty batch with valid token must return 200");
}
