use std::time::Duration;
use tokio::net::TcpStream;
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

#[tokio::test]
async fn agent_hello_gets_ready_and_registers() {
    let reg = altkey_relay::registry::Registry::new();
    let agent_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let agent_addr = agent_listener.local_addr().unwrap();
    {
        let reg = reg.clone();
        tokio::spawn(async move {
            let (sock, _) = agent_listener.accept().await.unwrap();
            altkey_relay::agent_conn::handle_for_test(reg, sock).await.ok();
        });
    }
    let mut c = TcpStream::connect(agent_addr).await.unwrap();
    write_msg(&mut c, &AgentMsg::Hello { handle: "h1".into(), token: "t".into() }).await.unwrap();
    let reply: RelayMsg = read_msg(&mut c).await.unwrap();
    assert_eq!(reply, RelayMsg::Ready);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(reg.control_for("h1").is_some(), "handle should be registered");
}
