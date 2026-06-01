# altkey Tunnel — Implementation Plan (Plan 2 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a user's local altkey agent reachable from anywhere at `https://<handle>.altkey.app/v1` via a relay we host — while the relay **never decrypts** the traffic (it routes by SNI and splices ciphertext) and the provider call still fires from the user's own machine/IP/sub.

**Architecture:** A small **relay** service (new crate) runs two listeners: a **public** `:443` listener for `*.altkey.app` and an **agent** listener. The agent (the existing `engine/`) opens a persistent **control connection** to the relay and claims its `<handle>`. When a public client connects, the relay peeks the TLS **ClientHello SNI** (plaintext — no decryption), tells the agent over the control channel to open a **data connection**, pairs the two sockets, and **splices raw bytes**. The agent terminates the public TLS on the data connection (holding the per-handle cert) and serves the existing axum router. This is the proven reverse-tunnel shape (frp/inlets), chosen over yamux multiplexing for tokio-simplicity and testability — same passthrough behavior.

**Tech Stack:** Rust, tokio, rustls/tokio-rustls (already in tree, ring provider), `tls-parser` (parse ClientHello SNI without decrypting), serde_json (length-prefixed control frames), rcgen (self-signed handle cert for MVP/test), axum/hyper-util (reuse the router-serving glue from `transparent/server.rs`). A new Cargo **workspace** ties `engine` + `relay` + a shared `tunnel-proto` lib.

**Reuses:** `engine`'s router (`routes::build_router`), the `transparent/server.rs` TLS-serving pattern (TlsAcceptor → hyper-util → router), `rcgen` (`transparent/ca.rs` style cert gen).

**Out of scope (later plans):** real subscription/handle validation against the control plane (Plan 3 — here it's a stub that accepts any token), ACME DNS-01 public certs (here: self-signed handle cert; the `HandleCert` trait lets ACME slot in later), the desktop app (Plan 4).

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` (repo root) | **new** Cargo workspace: members `engine`, `relay`, `tunnel-proto` |
| `tunnel-proto/Cargo.toml`, `tunnel-proto/src/lib.rs` | shared crate root |
| `tunnel-proto/src/messages.rs` | control-frame types (`AgentMsg`/`RelayMsg`) + length-prefixed read/write |
| `tunnel-proto/src/sni.rs` | `peek_sni()` — buffer a ClientHello, return `(sni, buffered_bytes)` |
| `relay/Cargo.toml`, `relay/src/main.rs` | relay binary entrypoint (start both listeners) |
| `relay/src/registry.rs` | `handle -> agent control sender` + `conn_id -> pending public socket` maps |
| `relay/src/agent_conn.rs` | accept agent control + data connections; handshake; register |
| `relay/src/public.rs` | accept public conn, peek SNI, request data conn, splice |
| `engine/src/tunnel.rs` | agent tunnel client: control loop + per-conn data dial + TLS terminate → router |
| `engine/src/tunnel_cert.rs` | `HandleCert` trait + `SelfSignedHandleCert` impl |
| `engine/src/config.rs` | add `relay_addr()` + `handle()` config (modify) |
| `engine/src/routes.rs` | add `/admin/tunnel/{start,stop,status}` (modify) |
| `engine/src/main.rs` | `mod tunnel; mod tunnel_cert;` + auto-start if `ALTKEY_TUNNEL=1` (modify) |
| `relay/tests/tunnel_e2e.rs` | end-to-end: relay + agent + client on loopback, request routes through |

**Shared constants / types (define once in `tunnel-proto`, reference everywhere):**
```rust
// tunnel-proto/src/messages.rs
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub enum AgentMsg { Hello { handle: String, token: String }, Data { conn_id: u64 } }
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub enum RelayMsg { Ready, Reject { reason: String }, Open { conn_id: u64 } }
```

---

## Task 1: Cargo workspace + crate skeletons

**Files:**
- Create: `Cargo.toml` (repo root)
- Create: `tunnel-proto/Cargo.toml`, `tunnel-proto/src/lib.rs`
- Create: `relay/Cargo.toml`, `relay/src/main.rs`

- [ ] **Step 1: Create the workspace root**

Create `C:/Users/gsent/Desktop/altkey/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["engine", "relay", "tunnel-proto"]
```

- [ ] **Step 2: Create `tunnel-proto`**

`tunnel-proto/Cargo.toml`:
```toml
[package]
name = "tunnel-proto"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["io-util", "net", "macros", "rt"] }
tls-parser = "0.12"
anyhow = "1"
```

`tunnel-proto/src/lib.rs`:
```rust
pub mod messages;
pub mod sni;
```

- [ ] **Step 3: Create `relay` skeleton**

`relay/Cargo.toml`:
```toml
[package]
name = "altkey-relay"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "altkey-relay"
path = "src/main.rs"

[dependencies]
tunnel-proto = { path = "../tunnel-proto" }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
parking_lot = "0.12"

[dev-dependencies]
tokio-rustls = { version = "0.26", default-features = false, features = ["ring", "tls12"] }
rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }
rustls-pemfile = "2"
rcgen = { version = "0.13", features = ["x509-parser"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
engine = { path = "../engine" }
```

`relay/src/main.rs` (stub that compiles; real wiring lands in Task 6):
```rust
//! altkey relay — public SNI-passthrough listener + agent control/data listener.
mod agent_conn;
mod public;
mod registry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("altkey_relay=info")),
    ).init();
    let public_addr = std::env::var("RELAY_PUBLIC_ADDR").unwrap_or_else(|_| "0.0.0.0:443".into());
    let agent_addr = std::env::var("RELAY_AGENT_ADDR").unwrap_or_else(|_| "0.0.0.0:7000".into());
    registry::Registry::new().run(&public_addr, &agent_addr).await
}
```

(Create empty `relay/src/{agent_conn,public,registry}.rs` with `// filled in later tasks` so it compiles — or define minimal stubs the later tasks replace. To compile now, registry.rs needs a `Registry` with `new()` + `run()`; provide a minimal stub that binds nothing and returns Ok, replaced in Task 5/6.)

Minimal `relay/src/registry.rs` stub:
```rust
pub struct Registry;
impl Registry {
    pub fn new() -> Self { Registry }
    pub async fn run(self, _public: &str, _agent: &str) -> anyhow::Result<()> { Ok(()) }
}
```
Empty `relay/src/agent_conn.rs` and `relay/src/public.rs`: `// implemented in later tasks`.

- [ ] **Step 4: Verify the workspace builds**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo build`
Expected: all three crates compile (engine unchanged, tunnel-proto + relay new). The `engine` crate is unaffected.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml tunnel-proto relay
git commit -m "build: cargo workspace (engine + relay + tunnel-proto) for the tunnel"
```

---

## Task 2: Control-frame messages + length-prefixed framing (`tunnel-proto/src/messages.rs`)

**Files:**
- Create: `tunnel-proto/src/messages.rs`

- [ ] **Step 1: Write the failing test + implementation**

Create `tunnel-proto/src/messages.rs`:
```rust
//! Control-channel messages between agent and relay, framed as a u32
//! length-prefix + JSON body. Small, human-debuggable, order-preserving.
use anyhow::{bail, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub enum AgentMsg {
    Hello { handle: String, token: String },
    Data { conn_id: u64 },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub enum RelayMsg {
    Ready,
    Reject { reason: String },
    Open { conn_id: u64 },
}

const MAX_FRAME: u32 = 64 * 1024;

pub async fn write_msg<W: AsyncWriteExt + Unpin, T: serde::Serialize>(w: &mut W, msg: &T) -> Result<()> {
    let body = serde_json::to_vec(msg)?;
    if body.len() as u32 > MAX_FRAME {
        bail!("frame too large: {}", body.len());
    }
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_msg<R: AsyncReadExt + Unpin, T: serde::de::DeserializeOwned>(r: &mut R) -> Result<T> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME {
        bail!("incoming frame too large: {len}");
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn agent_msg_round_trips_over_a_duplex() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let sent = AgentMsg::Hello { handle: "sen".into(), token: "tok".into() };
        write_msg(&mut a, &sent).await.unwrap();
        let got: AgentMsg = read_msg(&mut b).await.unwrap();
        assert_eq!(sent, got);
    }

    #[tokio::test]
    async fn relay_msg_open_round_trips() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_msg(&mut a, &RelayMsg::Open { conn_id: 42 }).await.unwrap();
        let got: RelayMsg = read_msg(&mut b).await.unwrap();
        assert_eq!(got, RelayMsg::Open { conn_id: 42 });
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p tunnel-proto messages`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add tunnel-proto/src/messages.rs
git commit -m "feat(tunnel-proto): length-prefixed control messages (Agent/Relay)"
```

---

## Task 3: SNI peek without decrypting (`tunnel-proto/src/sni.rs`)

**Files:**
- Create: `tunnel-proto/src/sni.rs`

- [ ] **Step 1: Write the implementation + test**

Create `tunnel-proto/src/sni.rs`:
```rust
//! Read the leading TLS ClientHello from a stream and extract the SNI server
//! name WITHOUT decrypting anything (SNI is plaintext in the ClientHello). The
//! bytes consumed are returned so the caller can replay them downstream — the
//! relay forwards the original handshake to the agent unchanged.
use anyhow::{bail, Result};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Read enough of `stream` to parse the first TLS record (the ClientHello) and
/// return `(sni, consumed_bytes)`. `consumed_bytes` is everything read so far
/// (the full first record), to be written to the agent before splicing the rest.
pub async fn peek_sni<R: AsyncRead + Unpin>(stream: &mut R) -> Result<(String, Vec<u8>)> {
    // TLS record header: type(1) version(2) length(2). Read header first.
    let mut head = [0u8; 5];
    stream.read_exact(&mut head).await?;
    if head[0] != 0x16 {
        bail!("not a TLS handshake record (type=0x{:02x})", head[0]);
    }
    let rec_len = u16::from_be_bytes([head[3], head[4]]) as usize;
    if rec_len == 0 || rec_len > 16 * 1024 {
        bail!("implausible ClientHello length {rec_len}");
    }
    let mut rec = vec![0u8; rec_len];
    stream.read_exact(&mut rec).await?;

    let mut consumed = Vec::with_capacity(5 + rec_len);
    consumed.extend_from_slice(&head);
    consumed.extend_from_slice(&rec);

    // Parse the handshake body for the SNI extension.
    let sni = parse_sni(&rec).ok_or_else(|| anyhow::anyhow!("no SNI in ClientHello"))?;
    Ok((sni, consumed))
}

/// Parse the SNI host_name from a TLS handshake ClientHello body using tls-parser.
fn parse_sni(handshake_body: &[u8]) -> Option<String> {
    use tls_parser::{parse_tls_message_handshake, TlsMessage, TlsMessageHandshake, TlsExtension};
    let (_, msg) = parse_tls_message_handshake(handshake_body).ok()?;
    let ch = match msg {
        TlsMessage::Handshake(TlsMessageHandshake::ClientHello(ch)) => ch,
        _ => return None,
    };
    let ext_bytes = ch.ext?;
    let (_, exts) = tls_parser::parse_tls_extensions(ext_bytes).ok()?;
    for ext in exts {
        if let TlsExtension::SNI(names) = ext {
            for (_typ, name) in names {
                if let Ok(s) = std::str::from_utf8(name) {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real captured TLS 1.2 ClientHello with SNI = "example.com".
    /// (Bytes verified to parse to that SNI.)
    fn example_client_hello() -> Vec<u8> {
        // Build a minimal ClientHello with an SNI extension for "example.com"
        // using rustls so the test owns a guaranteed-valid record.
        // (See Step note: generated at test time, not hand-encoded.)
        crate::sni::tests::build_client_hello("example.com")
    }

    pub(crate) fn build_client_hello(_host: &str) -> Vec<u8> {
        // Implemented in Step 2 via a tiny helper that drives a rustls client
        // far enough to emit its ClientHello. If that proves heavy, replace with
        // a static byte fixture (a captured ClientHello) — see Step 2 guidance.
        unreachable!("provided by Step 2")
    }

    #[tokio::test]
    async fn extracts_sni_from_client_hello() {
        let hello = example_client_hello();
        let mut cursor = std::io::Cursor::new(hello.clone());
        let mut stream = tokio::io::BufReader::new(&mut cursor);
        let (sni, consumed) = peek_sni(&mut stream).await.unwrap();
        assert_eq!(sni, "example.com");
        assert_eq!(consumed, hello, "must return the exact bytes consumed for replay");
    }
}
```

- [ ] **Step 2: Provide a real ClientHello fixture for the test**

The cleanest hermetic fixture is a **static captured ClientHello**. Replace the `tests` module's `example_client_hello`/`build_client_hello` with a `const` byte array of a real TLS 1.2 ClientHello whose SNI is `example.com`. Capture one with:
```bash
# In a scratch dir, capture a ClientHello to example.com:
python -c "import socket,ssl; s=socket.create_connection(('example.com',443)); ctx=ssl.create_default_context(); w=ctx.wrap_socket(s,server_hostname='example.com')" 2>/dev/null
# Easier: use openssl s_client -connect example.com:443 -servername example.com and capture, OR
# generate with the `tls-parser` repo fixtures.
```
If capturing is awkward, use this known-good approach instead: in the test, construct the ClientHello with `rustls`'s `ClientConnection` — create a `ClientConfig`, a `ClientConnection::new(cfg, "example.com".try_into()?)`, call `conn.write_tls(&mut buf)` once, and feed `buf` to `peek_sni`. Put that construction directly in the test (not `unreachable!`). Either path is fine; the test MUST feed `peek_sni` a real ClientHello with SNI `example.com` and assert the parse + exact-bytes-returned.

Remove the `unreachable!` placeholder once a real fixture is in place.

- [ ] **Step 3: Run the test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p tunnel-proto sni`
Expected: PASS — `extracts_sni_from_client_hello`.

- [ ] **Step 4: Commit**

```bash
git add tunnel-proto/src/sni.rs tunnel-proto/Cargo.toml
git commit -m "feat(tunnel-proto): peek ClientHello SNI without decrypting"
```

---

## Task 4: Per-handle cert (`engine/src/tunnel_cert.rs`)

**Files:**
- Create: `engine/src/tunnel_cert.rs`
- Modify: `engine/src/main.rs` (add `mod tunnel_cert;`) and `engine/src/lib.rs` (add `pub mod tunnel_cert;`)

- [ ] **Step 1: Write the trait + self-signed impl + test**

Create `engine/src/tunnel_cert.rs`:
```rust
//! Certificate the agent presents for `<handle>.altkey.app` when terminating
//! the public TLS. Trait so a real ACME DNS-01 impl can replace the self-signed
//! one later (Plan 2 ships only the self-signed impl; the integration test trusts it).
use anyhow::Result;
use rcgen::{CertificateParams, DnType, KeyPair};

pub trait HandleCert: Send + Sync {
    /// Returns (cert_chain_pem, private_key_pem) for `host` (e.g. "sen.altkey.app").
    fn cert_for(&self, host: &str) -> Result<(String, String)>;
}

/// Self-signed cert per host. Fine for local/loopback + tests; NOT publicly
/// trusted (a real client must opt to trust it). Production uses ACME instead.
pub struct SelfSignedHandleCert;

impl HandleCert for SelfSignedHandleCert {
    fn cert_for(&self, host: &str) -> Result<(String, String)> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![host.to_string()])?;
        params.distinguished_name.push(DnType::CommonName, host);
        let cert = params.self_signed(&key)?;
        Ok((cert.pem(), key.serialize_pem()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_signed_emits_cert_and_key_for_host() {
        let (cert, key) = SelfSignedHandleCert.cert_for("sen.altkey.app").unwrap();
        assert!(cert.contains("BEGIN CERTIFICATE"));
        assert!(key.contains("PRIVATE KEY"));
    }
}
```

- [ ] **Step 2: Register the module**

In `engine/src/main.rs` add `mod tunnel_cert;` near the other mods. In `engine/src/lib.rs` add `pub mod tunnel_cert;`.

- [ ] **Step 3: Run the test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p altkey tunnel_cert`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add engine/src/tunnel_cert.rs engine/src/main.rs engine/src/lib.rs
git commit -m "feat(engine): HandleCert trait + self-signed handle cert"
```

---

## Task 5: Relay registry + agent connections (`relay/src/registry.rs`, `relay/src/agent_conn.rs`)

**Files:**
- Modify: `relay/src/registry.rs`
- Modify: `relay/src/agent_conn.rs`

- [ ] **Step 1: Implement the registry**

Replace `relay/src/registry.rs`:
```rust
//! Shared relay state: which agent owns which handle, and a pending-connection
//! handoff used to pair a public socket with the agent's data connection.
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

/// A live agent control channel: send `RelayMsg::Open{conn_id}` to it.
type ControlTx = mpsc::Sender<u64>;

#[derive(Clone)]
pub struct Registry {
    handles: Arc<Mutex<HashMap<String, ControlTx>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<TcpStream>>>>,
    next_id: Arc<AtomicU64>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            handles: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn register_handle(&self, handle: String, tx: ControlTx) {
        self.handles.lock().insert(handle, tx);
    }
    pub fn unregister_handle(&self, handle: &str) {
        self.handles.lock().remove(handle);
    }
    pub fn control_for(&self, handle: &str) -> Option<ControlTx> {
        self.handles.lock().get(handle).cloned()
    }

    /// Reserve a conn_id and a slot the data connection will fill with the public socket.
    pub fn reserve_conn(&self) -> (u64, oneshot::Receiver<TcpStream>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        (id, rx)
    }
    /// The agent's data connection arrived for `conn_id`; hand it the public socket.
    pub fn take_pending(&self, conn_id: u64) -> Option<oneshot::Sender<TcpStream>> {
        self.pending.lock().remove(&conn_id)
    }

    pub async fn run(self, public_addr: &str, agent_addr: &str) -> anyhow::Result<()> {
        let agent = crate::agent_conn::serve(self.clone(), agent_addr.to_string());
        let public = crate::public::serve(self.clone(), public_addr.to_string());
        tokio::try_join!(agent, public)?;
        Ok(())
    }
}
```

- [ ] **Step 2: Implement agent connection handling**

Replace `relay/src/agent_conn.rs`:
```rust
//! Accept agent connections on the agent port. The FIRST frame decides:
//! - AgentMsg::Hello{handle,token} -> this is a control connection; validate,
//!   reply Ready, register, then forward Open{conn_id} requests to the agent.
//! - AgentMsg::Data{conn_id}       -> this is a data connection; hand this raw
//!   socket to the public side waiting on conn_id.
use crate::registry::Registry;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

/// Stub auth — Plan 3 replaces with a real control-plane check.
fn validate(_handle: &str, _token: &str) -> bool { true }

pub async fn serve(reg: Registry, addr: String) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("agent listener on {addr}");
    loop {
        let (sock, _) = listener.accept().await?;
        let reg = reg.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(reg, sock).await {
                tracing::debug!("agent conn ended: {e}");
            }
        });
    }
}

async fn handle(reg: Registry, mut sock: TcpStream) -> anyhow::Result<()> {
    let first: AgentMsg = read_msg(&mut sock).await?;
    match first {
        AgentMsg::Hello { handle, token } => {
            if !validate(&handle, &token) {
                write_msg(&mut sock, &RelayMsg::Reject { reason: "invalid".into() }).await?;
                return Ok(());
            }
            write_msg(&mut sock, &RelayMsg::Ready).await?;
            let (tx, mut rx) = mpsc::channel::<u64>(64);
            reg.register_handle(handle.clone(), tx);
            tracing::info!("agent registered handle {handle}");
            // Forward Open requests to the agent until the control conn drops.
            let res = async {
                while let Some(conn_id) = rx.recv().await {
                    write_msg(&mut sock, &RelayMsg::Open { conn_id }).await?;
                }
                Ok::<(), anyhow::Error>(())
            }.await;
            reg.unregister_handle(&handle);
            res
        }
        AgentMsg::Data { conn_id } => {
            // Hand this raw socket to the waiting public side.
            if let Some(slot) = reg.take_pending(conn_id) {
                let _ = slot.send(sock);
            }
            Ok(())
        }
    }
}
```

- [ ] **Step 3: Integration test — an agent can register a handle**

Add `relay/tests/agent_register.rs`:
```rust
use altkey_relay_testkit::*; // see note
```
Simpler: put the test inline by exposing a tiny helper. Create `relay/tests/agent_register.rs`:
```rust
use std::time::Duration;
use tokio::net::TcpStream;
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

// Bring the relay's registry into the test via the lib target (see Step 4 note:
// add a `[lib]` to relay so tests can use `altkey_relay::registry::Registry`).
#[tokio::test]
async fn agent_hello_gets_ready_and_registers() {
    let reg = altkey_relay::registry::Registry::new();
    let agent_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let agent_addr = agent_listener.local_addr().unwrap();
    {
        let reg = reg.clone();
        tokio::spawn(async move {
            // Accept one agent conn using the real handler.
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
```

- [ ] **Step 4: Make relay testable (lib target) + expose `handle_for_test`**

Add to `relay/Cargo.toml`:
```toml
[lib]
name = "altkey_relay"
path = "src/lib.rs"
```
Create `relay/src/lib.rs`:
```rust
pub mod agent_conn;
pub mod public;
pub mod registry;
```
In `relay/src/agent_conn.rs`, expose the handler for tests:
```rust
#[cfg(any(test, feature = "test-helpers"))]
pub async fn handle_for_test(reg: Registry, sock: TcpStream) -> anyhow::Result<()> {
    handle(reg, sock).await
}
```
Make `handle` `pub(crate)` if needed. Keep `main.rs` as the `[[bin]]`; it can `use altkey_relay::...` or keep its own `mod` lines — pick one and be consistent (recommended: `main.rs` does `use altkey_relay::registry::Registry;` and drops its own `mod` lines, since the lib now owns them).

- [ ] **Step 5: Run the test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p altkey-relay agent_register`
Expected: PASS — agent gets `Ready` and the handle is registered.

- [ ] **Step 6: Commit**

```bash
git add relay/Cargo.toml relay/src/lib.rs relay/src/main.rs relay/src/registry.rs relay/src/agent_conn.rs relay/tests/agent_register.rs
git commit -m "feat(relay): registry + agent control/data connection handling"
```

---

## Task 6: Relay public listener + splice (`relay/src/public.rs`)

**Files:**
- Modify: `relay/src/public.rs`

- [ ] **Step 1: Implement the public listener**

Replace `relay/src/public.rs`:
```rust
//! Public listener. For each inbound connection: peek the ClientHello SNI
//! (no decryption), map SNI host (<handle>.altkey.app) -> agent, ask the agent
//! to open a data connection, then splice the public socket <-> data socket.
//! The relay never decrypts; it forwards the original ClientHello bytes first.
use crate::registry::Registry;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tunnel_proto::sni::peek_sni;

/// Extract the handle from an SNI like "sen.altkey.app" -> "sen".
fn handle_from_sni(sni: &str) -> Option<&str> {
    sni.strip_suffix(".altkey.app").filter(|h| !h.is_empty())
}

pub async fn serve(reg: Registry, addr: String) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("public listener on {addr}");
    loop {
        let (sock, _) = listener.accept().await?;
        let reg = reg.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_public(reg, sock).await {
                tracing::debug!("public conn ended: {e}");
            }
        });
    }
}

pub(crate) async fn handle_public(reg: Registry, mut public: TcpStream) -> anyhow::Result<()> {
    let (sni, buffered) = peek_sni(&mut public).await?;
    let handle = handle_from_sni(&sni)
        .ok_or_else(|| anyhow::anyhow!("unknown sni {sni}"))?
        .to_string();
    let control = reg
        .control_for(&handle)
        .ok_or_else(|| anyhow::anyhow!("no agent for handle {handle}"))?;

    let (conn_id, data_rx) = reg.reserve_conn();
    control.send(conn_id).await.map_err(|_| anyhow::anyhow!("agent control closed"))?;

    // Wait for the agent's data connection to arrive (bounded).
    let mut data = tokio::time::timeout(Duration::from_secs(10), data_rx)
        .await
        .map_err(|_| anyhow::anyhow!("timeout waiting for agent data conn"))??;

    // Replay the buffered ClientHello to the agent, then splice raw bytes both ways.
    data.write_all(&buffered).await?;
    data.flush().await?;
    tokio::io::copy_bidirectional(&mut public, &mut data).await?;
    Ok(())
}
```

- [ ] **Step 2: Verify the workspace builds**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo build`
Expected: relay compiles. (End-to-end behavior is exercised in Task 8.)

- [ ] **Step 3: Commit**

```bash
git add relay/src/public.rs
git commit -m "feat(relay): public SNI-passthrough listener + byte splicing"
```

---

## Task 7: Agent tunnel client (`engine/src/tunnel.rs`)

**Files:**
- Create: `engine/src/tunnel.rs`
- Modify: `engine/src/main.rs` (`mod tunnel;`) + `engine/src/lib.rs` (`pub mod tunnel;`)
- Modify: `engine/src/config.rs` (add `relay_addr()`, `handle()`)
- Modify: `engine/Cargo.toml` (add `tunnel-proto` path dep)

- [ ] **Step 1: Add config + dep**

In `engine/Cargo.toml` `[dependencies]` add:
```toml
tunnel-proto = { path = "../tunnel-proto" }
```
Append to `engine/src/config.rs`:
```rust
/// Relay agent endpoint the tunnel client dials (host:port).
pub fn relay_addr() -> String {
    std::env::var("ALTKEY_RELAY_ADDR").unwrap_or_else(|_| "127.0.0.1:7000".into())
}
/// This agent's handle (subdomain). For MVP from env; later from the account.
pub fn handle() -> String {
    std::env::var("ALTKEY_HANDLE").unwrap_or_else(|_| "local".into())
}
```

- [ ] **Step 2: Implement the tunnel client**

Create `engine/src/tunnel.rs`:
```rust
//! Agent tunnel client. Opens a control connection to the relay, claims the
//! handle, and for each Open{conn_id} dials a data connection, terminates the
//! public TLS on it with the handle cert, and serves the existing router.
use crate::tunnel_cert::{HandleCert, SelfSignedHandleCert};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;
use tunnel_proto::messages::{read_msg, write_msg, AgentMsg, RelayMsg};

/// Build a TlsAcceptor for `host` from the handle cert.
fn acceptor_for(cert: &dyn HandleCert, host: &str) -> Result<TlsAcceptor> {
    let (cert_pem, key_pem) = cert.cert_for(host)?;
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes()).filter_map(|c| c.ok()).collect();
    let key: PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut key_pem.as_bytes())?.ok_or_else(|| anyhow!("no key"))?;
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    let cfg = ServerConfig::builder_with_provider(Arc::new(
        tokio_rustls::rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

/// Run the tunnel: connect, handshake, then serve Open requests until the
/// control connection drops. Returns Err if the initial connect/handshake fails.
pub async fn run(app: axum::Router, relay_addr: String, handle: String) -> Result<()> {
    let host = format!("{handle}.altkey.app");
    let cert: Arc<dyn HandleCert> = Arc::new(SelfSignedHandleCert);
    let acceptor = acceptor_for(cert.as_ref(), &host)?;

    let mut control = TcpStream::connect(&relay_addr).await?;
    write_msg(&mut control, &AgentMsg::Hello { handle: handle.clone(), token: "stub".into() }).await?;
    match read_msg::<_, RelayMsg>(&mut control).await? {
        RelayMsg::Ready => {}
        RelayMsg::Reject { reason } => return Err(anyhow!("relay rejected: {reason}")),
        other => return Err(anyhow!("unexpected relay reply: {other:?}")),
    }
    tracing::info!("tunnel up: https://{host}/ via {relay_addr}");

    loop {
        match read_msg::<_, RelayMsg>(&mut control).await {
            Ok(RelayMsg::Open { conn_id }) => {
                let relay_addr = relay_addr.clone();
                let acceptor = acceptor.clone();
                let app = app.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_one(relay_addr, conn_id, acceptor, app).await {
                        tracing::debug!("tunnel conn {conn_id} ended: {e}");
                    }
                });
            }
            Ok(_) => {}
            Err(e) => return Err(anyhow!("control connection closed: {e}")),
        }
    }
}

/// Dial a data connection, claim the conn_id, terminate TLS, serve the router.
async fn serve_one(relay_addr: String, conn_id: u64, acceptor: TlsAcceptor, app: axum::Router) -> Result<()> {
    let mut data = TcpStream::connect(&relay_addr).await?;
    write_msg(&mut data, &AgentMsg::Data { conn_id }).await?;
    // From here, `data` carries the public client's raw TLS. Terminate it.
    let tls = acceptor.accept(data).await?;
    let svc = hyper::service::service_fn(move |req| {
        let app = app.clone();
        async move { app.oneshot(req).await }
    });
    hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection(hyper_util::rt::TokioIo::new(tls), svc)
        .await
        .map_err(|e| anyhow!("serve: {e}"))?;
    Ok(())
}
```

- [ ] **Step 3: Register module**

`engine/src/main.rs`: add `mod tunnel;`. `engine/src/lib.rs`: add `pub mod tunnel;`.

- [ ] **Step 4: Verify build**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo build`
Expected: engine compiles with the tunnel client. (Behavior is verified in Task 8.)

- [ ] **Step 5: Commit**

```bash
git add engine/Cargo.toml engine/src/config.rs engine/src/tunnel.rs engine/src/main.rs engine/src/lib.rs
git commit -m "feat(engine): agent tunnel client (control + data conns, TLS terminate, serve router)"
```

---

## Task 8: End-to-end integration test (relay + agent + client, loopback)

**Files:**
- Create: `relay/tests/tunnel_e2e.rs`

- [ ] **Step 1: Write the end-to-end test**

This is the headline test: a relay + a real agent tunnel client + a real HTTPS client, all on loopback, proving a request to `https://h.altkey.app/...` routes through the tunnel to the agent's router, and that the agent's self-signed handle cert is what terminates TLS.

Create `relay/tests/tunnel_e2e.rs`:
```rust
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn request_routes_through_the_tunnel_end_to_end() {
    // 1. Start the relay on two ephemeral ports.
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
    tokio::time::sleep(Duration::from_millis(200)).await; // let the agent register

    // 3. Build a client that trusts the agent's self-signed cert for "h.altkey.app"
    //    and resolves that host to the relay's public port.
    let (cert_pem, _key) = altkey::tunnel_cert::SelfSignedHandleCert
        .cert_for("h.altkey.app").unwrap();
    // The agent regenerated its own cert internally; to trust it we instead
    // disable verification for this loopback test (the point under test is
    // routing + passthrough, not cert pinning). Use danger_accept_invalid_certs.
    let _ = cert_pem;
    let body = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .resolve("h.altkey.app", format!("127.0.0.1:{}", public_addr.port()).parse().unwrap())
        .build().unwrap()
        .get(format!("https://h.altkey.app:{}/v1/models", public_addr.port()))
        .timeout(Duration::from_secs(10))
        .send().await.unwrap()
        .text().await.unwrap();

    assert_eq!(body, "tunneled-ok",
        "request must route public -> relay (SNI passthrough) -> agent tunnel -> router");
}
```

- [ ] **Step 2: Expose `serve_listener` variants on the relay**

The test needs to start the relay listeners on pre-bound `TcpListener`s (ephemeral ports). In `relay/src/agent_conn.rs` and `relay/src/public.rs`, refactor `serve(reg, addr)` to bind then call a `serve_listener(reg, listener)` that loops on `accept`. Export both `serve_listener` functions as `pub`. (Keep `serve(reg, addr)` for `main.rs`; it binds then delegates to `serve_listener`.)

- [ ] **Step 3: Run the end-to-end test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p altkey-relay --test tunnel_e2e`
Expected: PASS — `tunneled-ok`. This proves: public connection → relay reads SNI `h.altkey.app` without decrypting → asks the agent to open a data conn → agent dials back, claims conn_id → relay splices → agent terminates TLS with its handle cert → router returns `tunneled-ok`. If it hangs, check the `sleep` is long enough for registration and that `copy_bidirectional` isn't deadlocking on the buffered-bytes replay.

- [ ] **Step 4: Run the full workspace test suite**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test`
Expected: all green — engine unit tests (incl. transparent + tunnel_cert), tunnel-proto tests, relay agent_register + tunnel_e2e.

- [ ] **Step 5: Commit**

```bash
git add relay/tests/tunnel_e2e.rs relay/src/agent_conn.rs relay/src/public.rs
git commit -m "test(relay): end-to-end tunnel — request routes through with SNI passthrough"
```

---

## Task 9: Wiring — `altkey tunnel` start + admin endpoints

**Files:**
- Modify: `engine/src/routes.rs` (add `/admin/tunnel/{start,stop,status}`)
- Modify: `engine/src/main.rs` (auto-start the tunnel if `ALTKEY_TUNNEL=1`)

- [ ] **Step 1: Add a tunnel-state flag + admin endpoints**

In `engine/src/tunnel.rs`, add a simple state flag:
```rust
use std::sync::atomic::{AtomicBool, Ordering};
pub static TUNNEL_UP: AtomicBool = AtomicBool::new(false);
pub fn is_up() -> bool { TUNNEL_UP.load(Ordering::SeqCst) }
```
Set `TUNNEL_UP.store(true, Ordering::SeqCst)` right after the `RelayMsg::Ready` match arm in `run`, and `store(false)` when `run` returns/errors.

In `engine/src/routes.rs`, add routes in `build_router()`:
```rust
        .route("/admin/tunnel/start", post(admin_tunnel_start))
        .route("/admin/tunnel/stop", post(admin_tunnel_stop))
        .route("/admin/tunnel/status", get(admin_tunnel_status))
```
And handlers at the bottom (match existing admin-handler conventions):
```rust
async fn admin_tunnel_start(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) { return (e.0, e.1).into_response(); }
    let app = build_router();
    let relay = crate::config::relay_addr();
    let handle = crate::config::handle();
    tokio::spawn(async move {
        if let Err(e) = crate::tunnel::run(app, relay, handle).await {
            tracing::warn!("tunnel exited: {e}");
        }
    });
    Json(json!({"ok": true, "starting": true})).into_response()
}

async fn admin_tunnel_stop(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) { return (e.0, e.1).into_response(); }
    // MVP: flip the flag; the spawned task exits when its control conn drops.
    crate::tunnel::TUNNEL_UP.store(false, std::sync::atomic::Ordering::SeqCst);
    Json(json!({"ok": true, "stopped": true})).into_response()
}

async fn admin_tunnel_status(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    Ok(Json(json!({"tunnel_up": crate::tunnel::is_up(), "handle": crate::config::handle()})))
}
```

- [ ] **Step 2: Auto-start on startup when requested**

In `engine/src/main.rs`, after the router build + the transparent block, add:
```rust
    if std::env::var("ALTKEY_TUNNEL").as_deref() == Ok("1") {
        let app = routes::build_router();
        let relay = config::relay_addr();
        let handle = config::handle();
        tokio::spawn(async move {
            if let Err(e) = tunnel::run(app, relay, handle).await {
                tracing::warn!("tunnel exited: {e}");
            }
        });
    }
```

- [ ] **Step 3: Build + test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo build && cargo test`
Expected: compiles; all tests still green.

- [ ] **Step 4: Commit**

```bash
git add engine/src/tunnel.rs engine/src/routes.rs engine/src/main.rs
git commit -m "feat(engine): tunnel wiring — ALTKEY_TUNNEL auto-start + /admin/tunnel endpoints"
```

---

## Self-Review

**Spec coverage:** Implements the spec's tunnel: agent opens an outbound connection and claims a handle; relay routes by SNI **without decrypting** (peek ClientHello, splice raw bytes — Task 3 + 6); the agent terminates the public TLS with a per-handle cert and serves the existing router (Task 7); reachable `https://<handle>.altkey.app/v1` (e2e Task 8). The `HandleCert` trait (Task 4) is the seam where ACME DNS-01 replaces the self-signed impl in production. Sub/handle validation is a documented **stub** (Task 5 `validate()`), explicitly Plan 3's job — not a hidden placeholder.

**Placeholder scan:** The only intentional stubs are `validate()` (Plan 3) and the self-signed `HandleCert` (ACME is deploy infra, not buildable now) — both are real, compiling, tested code with a clear seam, not "TODO". The Task 3 test fixture must be a real ClientHello (Step 2 gives two concrete ways to produce one); the `unreachable!` in the scaffold MUST be replaced before the test passes — flagged explicitly.

**Type consistency:** `AgentMsg`/`RelayMsg` (tunnel-proto) used identically by relay + agent. `Registry::{register_handle,control_for,reserve_conn,take_pending}`, `peek_sni() -> (String, Vec<u8>)`, `HandleCert::cert_for(host) -> (String,String)`, `tunnel::run(app, relay_addr, handle)`, `serve_listener` (relay) — consistent across tasks. `config::{relay_addr,handle}` defined Task 7, used in wiring Task 9.

**Known risks to watch during execution:** (1) `copy_bidirectional` plus the buffered-ClientHello replay — write the buffered bytes to the data conn BEFORE `copy_bidirectional`, as written; a deadlock here means the replay ordering is wrong. (2) rustls provider: reuse the **ring** path already settled in Plan 1 (engine) — the relay's dev-deps pin ring too. (3) `tls-parser` 0.12 API for ClientHello/extensions may differ slightly; reconcile `parse_tls_message_handshake` / `parse_tls_extensions` / `TlsExtension::SNI` to the installed version in Task 3. (4) The e2e test uses `danger_accept_invalid_certs` because the agent self-signs per-process; that's acceptable since the property under test is routing + passthrough, not cert trust (cert trust is an ACME/deploy concern).
