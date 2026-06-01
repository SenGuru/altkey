use std::sync::Arc;

#[tokio::test]
async fn intercept_server_serves_router_over_tls() {
    let app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(|| async { "ok-transparent" }),
    );
    let ca = Arc::new(altkey::transparent::ca::Ca::generate().unwrap());
    let server = altkey::transparent::server::serve_for_test(app, ca.clone(), 0)
        .await
        .expect("serve");
    let port = server.port;
    let body = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(ca.cert_pem.as_bytes()).unwrap())
        .resolve(
            "api.openai.com",
            format!("127.0.0.1:{port}").parse().unwrap(),
        )
        .build()
        .unwrap()
        .get(format!("https://api.openai.com:{port}/v1/models"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "ok-transparent");
    server.task.abort();
}
