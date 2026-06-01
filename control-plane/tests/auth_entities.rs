//! Migrate in-memory SQLite (both migrations) and round-trip one row per auth table.
use control_plane::entities::{identity, magic_link, oauth_flow, session};
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};

#[tokio::test]
async fn auth_tables_migrate_and_round_trip() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let now = chrono::Utc::now();
    let acct = uuid::Uuid::new_v4();

    identity::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(acct),
        provider: Set("github".into()),
        provider_user_id: Set("123".into()),
        email_at_provider: Set(Some("sen@example.com".into())),
        created_at: Set(now.into()),
    }
    .insert(&db)
    .await
    .unwrap();

    session::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(acct),
        token_hash: Set("hash1".into()),
        created_at: Set(now.into()),
        expires_at: Set((now + chrono::Duration::days(30)).into()),
        last_seen_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();

    magic_link::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        email: Set("sen@example.com".into()),
        token_hash: Set("hash2".into()),
        expires_at: Set((now + chrono::Duration::minutes(15)).into()),
        consumed_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();

    oauth_flow::ActiveModel {
        state: Set("state-abc".into()),
        provider: Set("google".into()),
        pkce_verifier: Set("verifier".into()),
        return_to: Set(None),
        expires_at: Set((now + chrono::Duration::minutes(10)).into()),
    }
    .insert(&db)
    .await
    .unwrap();
}
