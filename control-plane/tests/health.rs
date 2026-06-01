//! Boot the real app on an ephemeral port against in-memory SQLite and hit
//! /health and /api-docs/openapi.json over real HTTP.
use control_plane::app;
use control_plane::config::Config;
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::Database;

async fn boot() -> String {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let config = Config {
        database_url: "sqlite::memory:".into(),
        public_base_url: "http://127.0.0.1".into(),
        internal_service_secret: None,
        bind_addr: "127.0.0.1:0".into(),
    };
    let appx = app::build(AppState {
        db,
        config,
        email: std::sync::Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: std::sync::Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, appx).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn health_and_openapi_are_served() {
    let base = boot().await;
    let client = reqwest::Client::new();

    let h = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(h.status(), 200);
    let body: serde_json::Value = h.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["db"], true);

    let oa = client
        .get(format!("{base}/api-docs/openapi.json"))
        .send()
        .await
        .unwrap();
    assert_eq!(oa.status(), 200);
    let doc: serde_json::Value = oa.json().await.unwrap();
    assert_eq!(doc["info"]["title"], "altkey control plane");
    assert!(
        doc["paths"]["/health"].is_object(),
        "/health must be in the OpenAPI contract"
    );
}
