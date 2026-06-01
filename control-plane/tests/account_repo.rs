//! Boot an in-memory SQLite, run migrations, insert + fetch an account.
use control_plane::entities::{account, prelude::Account};
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, EntityTrait, Set};

#[tokio::test]
async fn migrate_then_insert_and_fetch_account() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let id = uuid::Uuid::new_v4();
    account::ActiveModel {
        id: Set(id),
        email: Set("sen@example.com".into()),
        display_name: Set(Some("Sen".into())),
        status: Set("active".into()),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let got = Account::find_by_id(id).one(&db).await.unwrap().unwrap();
    assert_eq!(got.email, "sen@example.com");
    assert_eq!(got.status, "active");
}
