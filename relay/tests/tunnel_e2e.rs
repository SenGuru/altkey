use std::time::Duration;

#[tokio::test]
async fn request_routes_through_the_tunnel_end_to_end() {
    // 1. Start the relay on two ephemeral ports using the real serve_listener fns.
    let reg = altkey_relay::registry::Registry::new();
    let public = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_addr = public.local_addr().unwrap();
    let agent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let agent_addr = agent.local_addr().unwrap();
    {
        let reg = reg.clone();
        tokio::spawn(async move { altkey_relay::agent_conn::serve_listener(reg, agent).await.ok(); });
    }
    {
        let reg = reg.clone();
        tokio::spawn(async move { altkey_relay::public::serve_listener(reg, public).await.ok(); });
    }

    // 2. Start the agent tunnel client with a tiny router + handle "h".
    let app = axum::Router::new().route("/v1/models", axum::routing::get(|| async { "tunneled-ok" }));
    {
        let app = app.clone();
        let agent_addr = agent_addr.to_string();
        tokio::spawn(async move { altkey::tunnel::run(app, agent_addr, "h".into()).await.ok(); });
    }

    // 3. Wait until the agent has registered the handle (poll, don't just sleep).
    for _ in 0..100 {
        if reg.control_for("h").is_some() { break; }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(reg.control_for("h").is_some(), "agent should register handle h");

    // 4. Client that trusts any cert (the agent self-signs per-process) and resolves
    //    h.altkey.app to the relay public port.
    let body = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .resolve("h.altkey.app", format!("127.0.0.1:{}", public_addr.port()).parse().unwrap())
        .build().unwrap()
        .get(format!("https://h.altkey.app:{}/v1/models", public_addr.port()))
        .timeout(Duration::from_secs(15))
        .send().await.unwrap()
        .text().await.unwrap();

    assert_eq!(body, "tunneled-ok",
        "request must route public -> relay (SNI passthrough) -> agent tunnel -> router");
}
