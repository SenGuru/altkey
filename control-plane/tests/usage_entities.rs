//! Migrate in-memory SQLite (all migrations) and round-trip one row per usage/adapter table.
use control_plane::entities::{adapter, usage_record, usage_rollup};
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, EntityTrait, Set};

#[tokio::test]
async fn usage_tables_migrate_and_round_trip() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let now = chrono::Utc::now();
    let account_id = uuid::Uuid::new_v4();
    let agent_id = uuid::Uuid::new_v4();
    let usage_id = uuid::Uuid::new_v4();

    // Round-trip: usage_record
    usage_record::ActiveModel {
        id: Set(usage_id),
        account_id: Set(account_id),
        agent_id: Set(Some(agent_id)),
        key_prefix: Set(Some("ak_live_abcd".into())),
        ts: Set(now.into()),
        provider: Set("openai".into()),
        model: Set("gpt-4o".into()),
        prompt_tokens: Set(100_i64),
        completion_tokens: Set(50_i64),
        total_tokens: Set(150_i64),
        tunnel_bytes: Set(2048_i64),
        tool: Set(Some("chat_completions".into())),
    }
    .insert(&db)
    .await
    .unwrap();

    // Verify it round-tripped
    let fetched = usage_record::Entity::find_by_id(usage_id)
        .one(&db)
        .await
        .unwrap()
        .expect("usage_record row should exist");
    assert_eq!(fetched.account_id, account_id);
    assert_eq!(fetched.agent_id, Some(agent_id));
    assert_eq!(fetched.prompt_tokens, 100_i64);
    assert_eq!(fetched.completion_tokens, 50_i64);
    assert_eq!(fetched.total_tokens, 150_i64);
    assert_eq!(fetched.tunnel_bytes, 2048_i64);
    assert_eq!(fetched.provider, "openai");
    assert_eq!(fetched.model, "gpt-4o");

    // Round-trip: usage_rollup
    let rollup_id = uuid::Uuid::new_v4();
    usage_rollup::ActiveModel {
        id: Set(rollup_id),
        account_id: Set(account_id),
        period: Set("2026-06-01".into()),
        model: Set(Some("gpt-4o".into())),
        tool: Set(Some("chat_completions".into())),
        provider: Set(Some("openai".into())),
        sum_requests: Set(5_i64),
        sum_tokens: Set(750_i64),
        sum_bytes: Set(10240_i64),
    }
    .insert(&db)
    .await
    .unwrap();

    let fetched_rollup = usage_rollup::Entity::find_by_id(rollup_id)
        .one(&db)
        .await
        .unwrap()
        .expect("usage_rollup row should exist");
    assert_eq!(fetched_rollup.account_id, account_id);
    assert_eq!(fetched_rollup.period, "2026-06-01");
    assert_eq!(fetched_rollup.sum_requests, 5_i64);
    assert_eq!(fetched_rollup.sum_tokens, 750_i64);
    assert_eq!(fetched_rollup.sum_bytes, 10240_i64);

    // Round-trip: adapter (with JSON manifest)
    let adapter_id = uuid::Uuid::new_v4();
    let manifest = serde_json::json!({"k": "v"});
    adapter::ActiveModel {
        id: Set(adapter_id),
        slug: Set("openai-base-url".into()),
        name: Set("OpenAI Base URL Shim".into()),
        description: Set("Rewrites the base URL for OpenAI-compatible endpoints".into()),
        version: Set("1.0.0".into()),
        target_tool: Set(Some("openai".into())),
        manifest: Set(manifest.clone()),
        published_at: Set(now.into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let fetched_adapter = adapter::Entity::find_by_id(adapter_id)
        .one(&db)
        .await
        .unwrap()
        .expect("adapter row should exist");
    assert_eq!(fetched_adapter.slug, "openai-base-url");
    assert_eq!(fetched_adapter.manifest, manifest);
    assert_eq!(fetched_adapter.manifest["k"], "v");
    assert_eq!(fetched_adapter.version, "1.0.0");
}
