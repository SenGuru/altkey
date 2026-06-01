//! active_subscription returns the row only when live + unexpired; upsert updates in place.
use control_plane::billing::plan::Plan;
use control_plane::billing::store::{active_subscription, upsert_from_polar, PolarSubscriptionEvent};
use migration::MigratorTrait;
use sea_orm::Database;

#[tokio::test]
async fn gate_reflects_status_and_expiry() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let acct = uuid::Uuid::new_v4();

    // Active, unexpired → gate open.
    upsert_from_polar(&db, &PolarSubscriptionEvent {
        account_id: acct,
        polar_customer_id: Some("cus".into()),
        polar_subscription_id: Some("sub".into()),
        plan: Plan::Standard,
        status: "active".into(),
        current_period_end: Some(chrono::Utc::now() + chrono::Duration::days(30)),
    }).await.unwrap();
    assert!(active_subscription(&db, acct).await.unwrap().is_some());

    // Canceled → gate closed (upsert updates the same row).
    upsert_from_polar(&db, &PolarSubscriptionEvent {
        account_id: acct,
        polar_customer_id: Some("cus".into()),
        polar_subscription_id: Some("sub".into()),
        plan: Plan::Standard,
        status: "canceled".into(),
        current_period_end: Some(chrono::Utc::now() + chrono::Duration::days(30)),
    }).await.unwrap();
    assert!(active_subscription(&db, acct).await.unwrap().is_none());

    // Active but expired → gate closed.
    upsert_from_polar(&db, &PolarSubscriptionEvent {
        account_id: acct,
        polar_customer_id: Some("cus".into()),
        polar_subscription_id: Some("sub".into()),
        plan: Plan::Pro,
        status: "active".into(),
        current_period_end: Some(chrono::Utc::now() - chrono::Duration::days(1)),
    }).await.unwrap();
    assert!(active_subscription(&db, acct).await.unwrap().is_none());

    // Unknown account → None.
    assert!(active_subscription(&db, uuid::Uuid::new_v4()).await.unwrap().is_none());
}
