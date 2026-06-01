//! Migrate in-memory SQLite (all migrations) and round-trip one row per registry table.
use control_plane::entities::{agent, endpoint_key, handle};
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};

#[tokio::test]
async fn registry_tables_migrate_and_round_trip() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let now = chrono::Utc::now();
    let account_id = uuid::Uuid::new_v4();
    let handle_id = uuid::Uuid::new_v4();
    let agent_id = uuid::Uuid::new_v4();

    // Round-trip: handle
    handle::ActiveModel {
        id: Set(handle_id),
        account_id: Set(account_id),
        name: Set("my-handle".into()),
        status: Set("active".into()),
        created_at: Set(now.into()),
    }
    .insert(&db)
    .await
    .unwrap();

    // Round-trip: agent
    agent::ActiveModel {
        id: Set(agent_id),
        account_id: Set(account_id),
        handle_id: Set(handle_id),
        name: Set("laptop".into()),
        agent_token_hash: Set("hash-of-ak-agent-token".into()),
        token_prefix: Set("ak_agent_abcd".into()),
        status: Set("active".into()),
        created_at: Set(now.into()),
        last_seen_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();

    // Round-trip: endpoint_key (with optional agent_id set)
    endpoint_key::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(account_id),
        agent_id: Set(Some(agent_id)),
        key_hash: Set("hash-of-ak-live-key".into()),
        key_prefix: Set("ak_live_abcd".into()),
        name: Set("prod key".into()),
        created_at: Set(now.into()),
        last_used_at: Set(None),
        revoked_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();

    // Round-trip: endpoint_key with null agent_id
    endpoint_key::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(account_id),
        agent_id: Set(None),
        key_hash: Set("hash-of-ak-live-key-2".into()),
        key_prefix: Set("ak_live_efgh".into()),
        name: Set("floating key".into()),
        created_at: Set(now.into()),
        last_used_at: Set(Some((now + chrono::Duration::hours(1)).into())),
        revoked_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();
}
