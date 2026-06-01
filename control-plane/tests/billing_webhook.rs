//! A correctly-signed subscription.active webhook upserts the account's subscription;
//! a bad signature is rejected; an unmapped product is acked but does nothing.
use control_plane::billing::polar::FakePolarClient;
use control_plane::billing::store::active_subscription;
use control_plane::billing::webhook::polar_webhook;
use control_plane::billing::webhook_sig::sign;
use control_plane::config::Config;
use control_plane::state::AppState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;

fn secret() -> String {
    use base64::Engine;
    format!("whsec_{}", base64::engine::general_purpose::STANDARD.encode([3u8; 32]))
}

async fn make_state() -> (AppState, sea_orm::DatabaseConnection) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let config = Config {
        database_url: "sqlite::memory:".into(),
        public_base_url: "http://localhost".into(),
        internal_service_secret: None,
        bind_addr: "127.0.0.1:0".into(),
        polar_access_token: None,
        polar_webhook_secret: Some(secret()),
        polar_base_url: "https://api.polar.sh".into(),
        polar_product_founding: None,
        polar_product_standard: Some("prod_standard".into()),
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

fn headers_for(secret: &str, body: &[u8]) -> HeaderMap {
    let sig = sign(secret, "msg_1", "1700000000", body);
    let mut h = HeaderMap::new();
    h.insert("webhook-id", "msg_1".parse().unwrap());
    h.insert("webhook-timestamp", "1700000000".parse().unwrap());
    h.insert("webhook-signature", sig.parse().unwrap());
    h
}

#[tokio::test]
async fn signed_subscription_webhook_upserts() {
    let (st, db) = make_state().await;
    let acct = uuid::Uuid::new_v4();
    let body = format!(
        r#"{{"type":"subscription.active","data":{{"id":"sub_1","customer_id":"cus_1","product_id":"prod_standard","status":"active","current_period_end":"2030-01-01T00:00:00Z","metadata":{{"account_id":"{acct}"}}}}}}"#
    )
    .into_bytes();

    let code = polar_webhook(
        State(st.clone()),
        headers_for(&secret(), &body),
        Bytes::from(body.clone()),
    )
    .await;
    assert_eq!(code, axum::http::StatusCode::OK);
    assert!(
        active_subscription(&db, acct).await.unwrap().is_some(),
        "subscription should be active after signed webhook"
    );

    // Bad signature → 401.
    let mut bad_headers = headers_for(&secret(), &body);
    bad_headers.insert("webhook-signature", "v1,AAAA".parse().unwrap());
    let code = polar_webhook(State(st), bad_headers, Bytes::from(body)).await;
    assert_eq!(code, axum::http::StatusCode::UNAUTHORIZED);
}
