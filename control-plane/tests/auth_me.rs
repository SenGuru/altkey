//! Boot the real app, manufacture a session for an account, and assert /me returns
//! that account with the cookie and 401 without it; logout then invalidates it.
use control_plane::app;
use control_plane::auth::session;
use control_plane::config::Config;
use control_plane::entities::account;
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};
use std::sync::Arc;

async fn boot() -> (String, sea_orm::DatabaseConnection) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let state = AppState {
        db: db.clone(),
        config: Config {
            database_url: "sqlite::memory:".into(),
            public_base_url: "http://127.0.0.1".into(),
            internal_service_secret: None,
            bind_addr: "127.0.0.1:0".into(),
            polar_access_token: None,
            polar_webhook_secret: None,
            polar_base_url: "https://api.polar.sh".into(),
            polar_product_founding: None,
            polar_product_standard: None,
            polar_product_pro: None,
        },
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
    };
    let appx = app::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, appx).await.unwrap() });
    (format!("http://{addr}"), db)
}

#[tokio::test]
async fn me_requires_session_and_logout_clears_it() {
    let (base, db) = boot().await;

    // Seed an account + a session token.
    let id = uuid::Uuid::new_v4();
    account::ActiveModel {
        id: Set(id),
        email: Set("sen@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();
    let token = session::issue(&db, id).await.unwrap();

    let client = reqwest::Client::new();

    // No cookie → 401.
    let r = client.get(format!("{base}/me")).send().await.unwrap();
    assert_eq!(r.status(), 401);

    // With cookie → 200 + the account.
    let r = client
        .get(format!("{base}/me"))
        .header("Cookie", format!("altkey_session={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["email"], "sen@example.com");

    // /me is present in the OpenAPI contract (router self-registration).
    let doc: serde_json::Value = client
        .get(format!("{base}/api-docs/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        doc["paths"]["/me"].is_object(),
        "/me must be in the served spec"
    );

    // Logout revokes the session.
    let r = client
        .post(format!("{base}/auth/logout"))
        .header("Cookie", format!("altkey_session={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // /me is now 401 — session was revoked by logout.
    let r = client
        .get(format!("{base}/me"))
        .header("Cookie", format!("altkey_session={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401, "session must be invalid after logout");
}
