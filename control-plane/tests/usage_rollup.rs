//! Tests for the usage rollup aggregation.
//! Inserts records across 2 days / 2 models for one account via store::insert_records,
//! calls rebuild_for_account, asserts rollups sum correctly, then re-runs to verify
//! idempotency (no doubling).
use altkey_api::dto::UsageRecordDto;
use chrono::{Duration, Utc};
use control_plane::billing::polar::FakePolarClient;
use control_plane::config::Config;
use control_plane::entities::{prelude::UsageRollup, usage_rollup};
use control_plane::usage::rollup::rebuild_for_account;
use control_plane::usage::store::insert_records;
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use uuid::Uuid;

async fn make_state() -> (AppState, sea_orm::DatabaseConnection) {
    let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
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
        polar_product_standard: None,
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

fn dto(ts: &str, model: &str, total_tokens: i64, tunnel_bytes: i64) -> UsageRecordDto {
    UsageRecordDto {
        ts: ts.to_string(),
        provider: "openai".into(),
        model: model.to_string(),
        prompt_tokens: total_tokens / 2,
        completion_tokens: total_tokens / 2,
        total_tokens,
        tunnel_bytes,
        tool: Some("chat".into()),
        key_prefix: None,
    }
}

#[tokio::test]
async fn rollup_sums_correctly_and_is_idempotent() {
    let (_st, db) = make_state().await;

    // Seed account
    let account_id = Uuid::new_v4();
    control_plane::entities::account::ActiveModel {
        id: Set(account_id),
        email: Set("rollup@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let day1 = Utc::now();
    let day2 = day1 - Duration::days(1);

    // Day 1, gpt-4o: 2 records → 400 tokens, 2048 bytes total
    let records = vec![
        dto(&day1.to_rfc3339(), "gpt-4o", 200, 1024),
        dto(&day1.to_rfc3339(), "gpt-4o", 200, 1024),
        // Day 1, gpt-3.5: 1 record → 100 tokens, 512 bytes
        dto(&day1.to_rfc3339(), "gpt-3.5-turbo", 100, 512),
        // Day 2, gpt-4o: 1 record → 300 tokens, 2048 bytes
        dto(&day2.to_rfc3339(), "gpt-4o", 300, 2048),
    ];

    let n = insert_records(&db, account_id, None, &records).await.unwrap();
    assert_eq!(n, 4, "all 4 records should be stored");

    // First rollup
    rebuild_for_account(&db, account_id).await.unwrap();

    let rollups = UsageRollup::find()
        .filter(usage_rollup::Column::AccountId.eq(account_id))
        .all(&db)
        .await
        .unwrap();

    // We expect 3 distinct rollup rows: (day1, gpt-4o), (day1, gpt-3.5-turbo), (day2, gpt-4o)
    assert_eq!(rollups.len(), 3, "should be 3 rollup rows; got {}", rollups.len());

    let day1_str = day1.format("%Y-%m-%d").to_string();
    let day2_str = day2.format("%Y-%m-%d").to_string();

    // (day1, gpt-4o): 2 requests, 400 tokens, 2048 bytes
    let r = rollups.iter().find(|r| r.period == day1_str && r.model.as_deref() == Some("gpt-4o")).unwrap();
    assert_eq!(r.sum_requests, 2, "day1/gpt-4o requests");
    assert_eq!(r.sum_tokens, 400, "day1/gpt-4o tokens");
    assert_eq!(r.sum_bytes, 2048, "day1/gpt-4o bytes");

    // (day1, gpt-3.5): 1 request, 100 tokens, 512 bytes
    let r = rollups.iter().find(|r| r.period == day1_str && r.model.as_deref() == Some("gpt-3.5-turbo")).unwrap();
    assert_eq!(r.sum_requests, 1, "day1/gpt-3.5 requests");
    assert_eq!(r.sum_tokens, 100, "day1/gpt-3.5 tokens");
    assert_eq!(r.sum_bytes, 512, "day1/gpt-3.5 bytes");

    // (day2, gpt-4o): 1 request, 300 tokens, 2048 bytes
    let r = rollups.iter().find(|r| r.period == day2_str && r.model.as_deref() == Some("gpt-4o")).unwrap();
    assert_eq!(r.sum_requests, 1, "day2/gpt-4o requests");
    assert_eq!(r.sum_tokens, 300, "day2/gpt-4o tokens");
    assert_eq!(r.sum_bytes, 2048, "day2/gpt-4o bytes");

    // ── Idempotency: run rebuild again and counts must NOT double ────────────
    rebuild_for_account(&db, account_id).await.unwrap();

    let rollups2 = UsageRollup::find()
        .filter(usage_rollup::Column::AccountId.eq(account_id))
        .all(&db)
        .await
        .unwrap();

    assert_eq!(rollups2.len(), 3, "rollup count must not change on second rebuild");

    let r2 = rollups2.iter().find(|r| r.period == day1_str && r.model.as_deref() == Some("gpt-4o")).unwrap();
    assert_eq!(r2.sum_requests, 2, "idempotent: day1/gpt-4o requests must not double");
    assert_eq!(r2.sum_tokens, 400, "idempotent: day1/gpt-4o tokens must not double");
}

#[tokio::test]
async fn rollup_isolation_between_accounts() {
    let (_st, db) = make_state().await;

    let acct1 = Uuid::new_v4();
    let acct2 = Uuid::new_v4();

    for (id, email) in [(acct1, "acct1@example.com"), (acct2, "acct2@example.com")] {
        control_plane::entities::account::ActiveModel {
            id: Set(id),
            email: Set(email.into()),
            display_name: Set(None),
            status: Set("active".into()),
            created_at: Set(Utc::now().into()),
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let now = Utc::now().to_rfc3339();
    insert_records(&db, acct1, None, &[dto(&now, "gpt-4o", 200, 1024)]).await.unwrap();
    insert_records(&db, acct2, None, &[dto(&now, "gpt-4o", 500, 4096)]).await.unwrap();

    rebuild_for_account(&db, acct1).await.unwrap();
    rebuild_for_account(&db, acct2).await.unwrap();

    let rows1 = UsageRollup::find()
        .filter(usage_rollup::Column::AccountId.eq(acct1))
        .all(&db)
        .await
        .unwrap();
    let rows2 = UsageRollup::find()
        .filter(usage_rollup::Column::AccountId.eq(acct2))
        .all(&db)
        .await
        .unwrap();

    assert_eq!(rows1.len(), 1);
    assert_eq!(rows1[0].sum_tokens, 200, "acct1 tokens");
    assert_eq!(rows2.len(), 1);
    assert_eq!(rows2[0].sum_tokens, 500, "acct2 tokens should be independent");
}
