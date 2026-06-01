//! Validate the OAuth flow's state handling without hitting a real provider:
//! an unknown provider 404s; a callback with an unknown state is rejected.
use control_plane::auth::oauth::{callback, start, CallbackQuery, OAuthRegistry, Provider};
use control_plane::config::Config;
use control_plane::state::AppState;
use axum::extract::{Path, Query, State};
use axum_extra::extract::cookie::CookieJar;
use migration::MigratorTrait;
use sea_orm::Database;
use std::collections::HashMap;
use std::sync::Arc;

async fn state_with(provider: Option<Provider>) -> AppState {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let mut providers = HashMap::new();
    if let Some(p) = provider {
        providers.insert(p.name.clone(), p);
    }
    AppState {
        db,
        config: Config {
            database_url: "sqlite::memory:".into(),
            public_base_url: "http://localhost".into(),
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
        oauth: Arc::new(OAuthRegistry { providers }),
        polar: Arc::new(control_plane::billing::polar::FakePolarClient),
    }
}

#[tokio::test]
async fn unknown_provider_is_404() {
    let st = state_with(None).await;
    let r = start(State(st), Path("nope".into())).await;
    assert!(r.is_err(), "unknown provider must 404");
}

#[tokio::test]
async fn callback_with_unknown_state_is_rejected() {
    let p = Provider {
        name: "google".into(),
        client_id: "id".into(),
        client_secret: "secret".into(),
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo".into(),
        scopes: vec!["openid".into()],
        id_field: "sub".into(),
        email_field: "email".into(),
    };
    let st = state_with(Some(p)).await;
    let r = callback(
        State(st),
        Path("google".into()),
        CookieJar::new(),
        Query(CallbackQuery {
            code: "x".into(),
            state: "never-stored".into(),
        }),
    )
    .await;
    assert!(r.is_err(), "callback with an unknown state must be rejected");
}
