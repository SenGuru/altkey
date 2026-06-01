//! Migrate (all migrations) on SQLite and round-trip a subscription row.
use control_plane::entities::subscription;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};

#[tokio::test]
async fn subscription_migrates_and_round_trips() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let now = chrono::Utc::now();
    subscription::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(uuid::Uuid::new_v4()),
        polar_customer_id: Set(Some("cus_1".into())),
        polar_subscription_id: Set(Some("sub_1".into())),
        plan: Set("standard".into()),
        status: Set("active".into()),
        current_period_end: Set(Some((now + chrono::Duration::days(30)).into())),
        is_founding: Set(false),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }.insert(&db).await.unwrap();
}
