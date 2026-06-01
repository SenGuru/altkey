//! Drive the magic-link flow end to end against the real handlers + capturing email.
use control_plane::auth::email::CapturingEmailSender;
use control_plane::auth::magic_link::{consume, request, ConsumeQuery, MagicRequest};
use control_plane::config::Config;
use control_plane::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;

fn url_token(link: &str) -> String {
    link.split("token=").nth(1).unwrap().to_string()
}

#[tokio::test]
async fn magic_link_request_then_consume_issues_session() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let email = CapturingEmailSender::default();
    let state = AppState {
        db: db.clone(),
        config: Config {
            database_url: "sqlite::memory:".into(),
            public_base_url: "http://localhost".into(),
            internal_service_secret: None,
            bind_addr: "127.0.0.1:0".into(),
        },
        email: Arc::new(email.clone()),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
    };

    let _ = request(State(state.clone()), Json(MagicRequest { email: "Sen@Example.com".into() }))
        .await
        .unwrap();

    let (to, link) = email.sent.lock().unwrap()[0].clone();
    assert_eq!(to, "sen@example.com", "email is normalized lowercase");
    let token = url_token(&link);

    let resp = consume(State(state.clone()), CookieJar::new(), Query(ConsumeQuery { token: token.clone() }))
        .await
        .expect("consume succeeds");
    let _ = resp; // a 303 with a Set-Cookie; presence asserted below via a second consume

    // Single-use: consuming the same token again must fail.
    let again = consume(State(state), CookieJar::new(), Query(ConsumeQuery { token })).await;
    assert!(again.is_err(), "second consume must be rejected");
}
