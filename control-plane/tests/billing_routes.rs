//! Boot the app, sign in (seed a session), and hit /billing/checkout + /billing/subscription.
use control_plane::app;
use control_plane::auth::session;
use control_plane::billing::plan::Plan;
use control_plane::billing::store::{upsert_from_polar, PolarSubscriptionEvent};
use control_plane::config::Config;
use control_plane::entities::account;
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};
use std::sync::Arc;

async fn boot() -> (String, sea_orm::DatabaseConnection, uuid::Uuid, String) {
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
    let state = AppState {
        db: db.clone(),
        config,
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: Arc::new(control_plane::billing::polar::FakePolarClient),
    };
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
    let appx = app::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, appx).await.unwrap() });
    (format!("http://{addr}"), db, id, token)
}

#[tokio::test]
async fn checkout_and_subscription_views() {
    let (base, db, acct, token) = boot().await;
    let client = reqwest::Client::new();

    // checkout returns a (fake) Polar URL embedding the account id.
    let r = client
        .post(format!("{base}/billing/checkout"))
        .header("Cookie", format!("altkey_session={token}"))
        .json(&serde_json::json!({ "plan": "standard" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert!(
        v["url"].as_str().unwrap().contains(&acct.to_string()),
        "checkout URL must embed the account id"
    );

    // No subscription yet → active=false.
    let r = client
        .get(format!("{base}/billing/subscription"))
        .header("Cookie", format!("altkey_session={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["active"], false);

    // After a webhook-style upsert → active=true.
    upsert_from_polar(
        &db,
        &PolarSubscriptionEvent {
            account_id: acct,
            polar_customer_id: Some("cus".into()),
            polar_subscription_id: Some("sub".into()),
            plan: Plan::Standard,
            status: "active".into(),
            current_period_end: Some(chrono::Utc::now() + chrono::Duration::days(30)),
        },
    )
    .await
    .unwrap();
    let r = client
        .get(format!("{base}/billing/subscription"))
        .header("Cookie", format!("altkey_session={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["active"], true);
    assert_eq!(v["plan"], "standard");

    // /billing/subscription is in the served contract.
    let doc: serde_json::Value = client
        .get(format!("{base}/api-docs/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        doc["paths"]["/billing/subscription"].is_object(),
        "/billing/subscription must appear in the OpenAPI spec"
    );
}
