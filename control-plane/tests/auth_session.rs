//! Issue a session, resolve it back to the account, then revoke it.
use control_plane::auth::session;
use control_plane::entities::account;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};

#[tokio::test]
async fn session_issue_lookup_revoke() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let id = uuid::Uuid::new_v4();
    account::ActiveModel {
        id: Set(id),
        email: Set("sen@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(chrono::Utc::now().into()),
    }.insert(&db).await.unwrap();

    let token = session::issue(&db, id).await.unwrap();
    let got = session::account_for(&db, &token).await.unwrap();
    assert_eq!(got.unwrap().id, id);

    session::revoke(&db, &token).await.unwrap();
    assert!(session::account_for(&db, &token).await.unwrap().is_none());

    // An unknown token resolves to None, not an error.
    assert!(session::account_for(&db, "bogus").await.unwrap().is_none());
}
