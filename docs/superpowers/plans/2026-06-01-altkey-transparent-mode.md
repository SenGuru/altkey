# altkey Transparent Mode — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the altkey key work *out of the box* on the local machine — a tool sends to `api.openai.com` / `api.anthropic.com` as usual and altkey transparently serves it from the user's subscription, with **zero base-URL config and no per-tool patching.**

**Architecture:** Extend the existing Rust agent (`engine/`). Add a `transparent/` module that (1) generates a local root CA + per-host leaf certs, (2) installs the CA into the OS trust store, (3) redirects `api.openai.com` + `api.anthropic.com` to `127.0.0.1` via the hosts file, and (4) runs an HTTPS listener on **:443** that terminates TLS (SNI-selected leaf cert) and routes the intercepted hosts into the **existing** axum router (`/v1/*`). Per-request key validation is already bypassed when `ALTKEY_TRANSPARENT=1` (existing code), so the tool's own key is ignored and the sub is used.

**Tech Stack:** Rust, axum 0.7, tokio, tokio-rustls + rustls (server TLS), rcgen 0.13 (cert generation), rustls-pemfile. OS trust-store + hosts edits via std fs + shelling to `certutil` (Windows) / `security` (macOS) / `update-ca-certificates` (Linux).

**Reuses (already built):** `engine/src/routes.rs` router + all `/v1/*` handlers; the `ALTKEY_TRANSPARENT` per-request bypass in `routes.rs::auth_key`; `config.rs` paths.

---

## File Structure

| File | Responsibility |
|---|---|
| `engine/Cargo.toml` | add `rcgen`, `tokio-rustls`, `rustls-pemfile` (modify) |
| `engine/src/transparent/mod.rs` | orchestrator: `enable()` / `disable()` / `status()` |
| `engine/src/transparent/ca.rs` | generate/load root CA; mint per-host leaf certs |
| `engine/src/transparent/hosts.rs` | add/remove `127.0.0.1 <host>` entries (cross-platform, marker-bounded) |
| `engine/src/transparent/trust.rs` | install/uninstall the CA in the OS trust store |
| `engine/src/transparent/server.rs` | the :443 HTTPS listener with SNI cert resolver → existing router |
| `engine/src/config.rs` | add cert/CA paths + intercepted-host list (modify) |
| `engine/src/routes.rs` | add `/admin/transparent/{enable,disable,status}` (modify) |
| `engine/src/main.rs` | `mod transparent;` + optional auto-enable on startup flag (modify) |
| `engine/tests/transparent_server.rs` | integration test: HTTPS to `api.openai.com` → routed |

**Constants used across tasks (define once, reference everywhere):**
- Intercepted hosts: `const INTERCEPT_HOSTS: [&str; 2] = ["api.openai.com", "api.anthropic.com"];`
- CA paths: `config::ca_cert_path()` → `~/.altkey/ca.crt`, `config::ca_key_path()` → `~/.altkey/ca.key`.
- Hosts markers: `# >>> altkey transparent >>>` … `# <<< altkey transparent <<<`.

---

## Task 1: Add dependencies

**Files:**
- Modify: `engine/Cargo.toml`

- [ ] **Step 1: Add the crates**

In `engine/Cargo.toml`, under `[dependencies]`, add:

```toml
rcgen = "0.13"
tokio-rustls = "0.26"
rustls-pemfile = "2"
```

(`rustls` is already present transitively via reqwest; `tokio-rustls` 0.26 pairs with rustls 0.23.)

- [ ] **Step 2: Verify it builds**

Run: `cd engine && cargo build`
Expected: compiles (downloads rcgen/tokio-rustls), no errors.

- [ ] **Step 3: Commit**

```bash
git add engine/Cargo.toml engine/Cargo.lock
git commit -m "build(engine): add rcgen + tokio-rustls for transparent mode"
```

---

## Task 2: Config paths + intercept-host constant

**Files:**
- Modify: `engine/src/config.rs`

- [ ] **Step 1: Add the paths + constant**

Append to `engine/src/config.rs`:

```rust
/// Hosts altkey intercepts in transparent mode.
pub const INTERCEPT_HOSTS: [&str; 2] = ["api.openai.com", "api.anthropic.com"];

pub fn ca_cert_path() -> PathBuf {
    altkey_dir().join("ca.crt")
}

pub fn ca_key_path() -> PathBuf {
    altkey_dir().join("ca.key")
}

/// Path to the OS hosts file.
pub fn hosts_path() -> PathBuf {
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        std::path::Path::new(&root).join("System32\\drivers\\etc\\hosts")
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from("/etc/hosts")
    }
}
```

- [ ] **Step 2: Verify it builds**

Run: `cd engine && cargo build`
Expected: compiles, no errors.

- [ ] **Step 3: Commit**

```bash
git add engine/src/config.rs
git commit -m "feat(engine): config paths + intercept-host list for transparent mode"
```

---

## Task 3: Local CA + leaf certs (`transparent/ca.rs`)

**Files:**
- Create: `engine/src/transparent/mod.rs` (stub for now)
- Create: `engine/src/transparent/ca.rs`
- Modify: `engine/src/main.rs` (add `mod transparent;`)

- [ ] **Step 1: Register the module**

In `engine/src/main.rs`, add near the other `mod` lines:

```rust
mod transparent;
```

Create `engine/src/transparent/mod.rs` with:

```rust
pub mod ca;
```

- [ ] **Step 2: Write the failing test**

Create `engine/src/transparent/ca.rs`:

```rust
//! Local root CA + per-host leaf certificate generation for transparent mode.
//! The CA is generated once and stored at ~/.altkey/ca.{crt,key}. Leaf certs for
//! intercepted hosts (api.openai.com etc.) are minted on demand, signed by the CA.
use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use std::path::Path;

/// An in-memory CA: the self-signed cert PEM + its key pair.
pub struct Ca {
    pub cert_pem: String,
    key: KeyPair,
    cert: rcgen::Certificate,
}

/// A minted leaf: cert chain PEM (leaf only) + private key PEM.
pub struct Leaf {
    pub cert_pem: String,
    pub key_pem: String,
}

impl Ca {
    /// Generate a fresh root CA.
    pub fn generate() -> Result<Ca> {
        let key = KeyPair::generate().context("ca keypair")?;
        let mut params = CertificateParams::new(Vec::<String>::new()).context("ca params")?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        params
            .distinguished_name
            .push(DnType::CommonName, "altkey local CA");
        params
            .distinguished_name
            .push(DnType::OrganizationName, "altkey");
        let cert = params.self_signed(&key).context("self-sign ca")?;
        Ok(Ca {
            cert_pem: cert.pem(),
            key,
            cert,
        })
    }

    /// Mint a leaf cert for `host`, signed by this CA.
    pub fn mint_leaf(&self, host: &str) -> Result<Leaf> {
        let leaf_key = KeyPair::generate().context("leaf keypair")?;
        let mut params =
            CertificateParams::new(vec![host.to_string()]).context("leaf params")?;
        params
            .distinguished_name
            .push(DnType::CommonName, host);
        let leaf = params
            .signed_by(&leaf_key, &self.cert, &self.key)
            .context("sign leaf")?;
        Ok(Leaf {
            cert_pem: leaf.pem(),
            key_pem: leaf_key.serialize_pem(),
        })
    }

    /// Persist the CA to disk (cert + key).
    pub fn save(&self, cert_path: &Path, key_path: &Path) -> Result<()> {
        std::fs::write(cert_path, &self.cert_pem).context("write ca cert")?;
        std::fs::write(key_path, self.key.serialize_pem()).context("write ca key")?;
        Ok(())
    }

    /// Load an existing CA, or generate + save a new one.
    pub fn load_or_create(cert_path: &Path, key_path: &Path) -> Result<Ca> {
        if cert_path.exists() && key_path.exists() {
            let key_pem = std::fs::read_to_string(key_path).context("read ca key")?;
            let key = KeyPair::from_pem(&key_pem).context("parse ca key")?;
            let cert_pem = std::fs::read_to_string(cert_path).context("read ca cert")?;
            // Re-parse params from the stored cert to reconstruct a signer.
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)
                .context("parse ca cert")?;
            let cert = params.self_signed(&key).context("rebuild ca")?;
            Ok(Ca { cert_pem, key, cert })
        } else {
            let ca = Ca::generate()?;
            ca.save(cert_path, key_path)?;
            Ok(ca)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_generates_and_mints_leaf_with_san() {
        let ca = Ca::generate().unwrap();
        assert!(ca.cert_pem.contains("BEGIN CERTIFICATE"));
        let leaf = ca.mint_leaf("api.openai.com").unwrap();
        assert!(leaf.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(leaf.key_pem.contains("PRIVATE KEY"));
        // The leaf must be parseable as a valid cert.
        let parsed = CertificateParams::from_ca_cert_pem(&leaf.cert_pem);
        assert!(parsed.is_ok(), "leaf cert should parse");
    }

    #[test]
    fn ca_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("altkey-ca-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert_p = dir.join("ca.crt");
        let key_p = dir.join("ca.key");
        let ca1 = Ca::load_or_create(&cert_p, &key_p).unwrap();
        let ca2 = Ca::load_or_create(&cert_p, &key_p).unwrap();
        // Same persisted cert PEM on reload.
        assert_eq!(ca1.cert_pem, ca2.cert_pem);
        // Reloaded CA can still mint a leaf.
        assert!(ca2.mint_leaf("api.anthropic.com").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 3: Run the test to verify it fails (not yet compiled into the build)**

Run: `cd engine && cargo test transparent::ca`
Expected: FAIL — first run may fail to compile until `mod transparent;` + `pub mod ca;` are wired (Step 1). If Step 1 done, it should compile and the tests should PASS (the implementation is included above). If any rcgen API mismatch appears, fix the call to match the installed rcgen 0.13 signatures before proceeding.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd engine && cargo test transparent::ca`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add engine/src/main.rs engine/src/transparent/mod.rs engine/src/transparent/ca.rs
git commit -m "feat(engine): local CA + per-host leaf cert generation (transparent mode)"
```

---

## Task 4: Hosts-file management (`transparent/hosts.rs`)

**Files:**
- Create: `engine/src/transparent/hosts.rs`
- Modify: `engine/src/transparent/mod.rs` (add `pub mod hosts;`)

- [ ] **Step 1: Register the module**

Add to `engine/src/transparent/mod.rs`:

```rust
pub mod hosts;
```

- [ ] **Step 2: Write the failing test + implementation**

Create `engine/src/transparent/hosts.rs`:

```rust
//! Add/remove `127.0.0.1 <host>` redirect entries in the OS hosts file.
//! All altkey entries live inside a single marker-bounded block so removal is
//! exact and idempotent. Editing the real hosts file needs admin/root; the pure
//! text transform is unit-tested against an in-memory string.
use anyhow::{Context, Result};
use std::path::Path;

const BEGIN: &str = "# >>> altkey transparent >>>";
const END: &str = "# <<< altkey transparent <<<";

/// Return `content` with the altkey block (re)written to redirect `hosts`.
/// Pure function — no I/O — so it is fully unit-testable.
pub fn apply_block(content: &str, hosts: &[&str]) -> String {
    let stripped = strip_block(content);
    let mut out = stripped.trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(BEGIN);
    out.push('\n');
    for h in hosts {
        out.push_str(&format!("127.0.0.1 {h}\n"));
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// Return `content` with any altkey block removed.
pub fn strip_block(content: &str) -> String {
    let (Some(b), Some(e)) = (content.find(BEGIN), content.find(END)) else {
        return content.to_string();
    };
    if e < b {
        return content.to_string();
    }
    let end_idx = e + END.len();
    let mut result = String::new();
    result.push_str(&content[..b]);
    // Skip the trailing newline after END if present.
    let rest = &content[end_idx..];
    result.push_str(rest.strip_prefix('\n').unwrap_or(rest));
    result.trim_end().to_string() + "\n"
}

/// Write the redirect block into the real hosts file at `path`.
pub fn enable(path: &Path, hosts: &[&str]) -> Result<()> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let next = apply_block(&current, hosts);
    std::fs::write(path, next).with_context(|| format!("writing hosts {}", path.display()))?;
    Ok(())
}

/// Remove the altkey block from the real hosts file at `path`.
pub fn disable(path: &Path) -> Result<()> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let next = strip_block(&current);
    std::fs::write(path, next).with_context(|| format!("writing hosts {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_then_strip_is_identity() {
        let original = "127.0.0.1 localhost\n::1 localhost\n";
        let with = apply_block(original, &["api.openai.com", "api.anthropic.com"]);
        assert!(with.contains("127.0.0.1 api.openai.com"));
        assert!(with.contains("127.0.0.1 api.anthropic.com"));
        assert!(with.contains(BEGIN) && with.contains(END));
        // Original lines are preserved.
        assert!(with.contains("127.0.0.1 localhost"));
        let back = strip_block(&with);
        assert!(!back.contains("api.openai.com"));
        assert!(!back.contains(BEGIN));
        assert!(back.contains("127.0.0.1 localhost"));
    }

    #[test]
    fn apply_is_idempotent() {
        let original = "127.0.0.1 localhost\n";
        let once = apply_block(original, &["api.openai.com"]);
        let twice = apply_block(&once, &["api.openai.com"]);
        assert_eq!(once, twice, "applying twice must not stack blocks");
        assert_eq!(twice.matches(BEGIN).count(), 1);
    }
}
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cd engine && cargo test transparent::hosts`
Expected: PASS (2 tests). (Implementation is included; these tests touch no real files.)

- [ ] **Step 4: Commit**

```bash
git add engine/src/transparent/mod.rs engine/src/transparent/hosts.rs
git commit -m "feat(engine): marker-bounded hosts-file redirect (transparent mode)"
```

---

## Task 5: OS trust-store install (`transparent/trust.rs`)

**Files:**
- Create: `engine/src/transparent/trust.rs`
- Modify: `engine/src/transparent/mod.rs` (add `pub mod trust;`)

- [ ] **Step 1: Register the module**

Add to `engine/src/transparent/mod.rs`:

```rust
pub mod trust;
```

- [ ] **Step 2: Write the command-builder test + implementation**

The actual install touches the system trust store (needs elevation) so it is integration/manual-tested, but the **command construction** is pure and unit-tested. Create `engine/src/transparent/trust.rs`:

```rust
//! Install / uninstall the altkey local CA into the OS trust store.
//! The system command differs per OS; we build it as an argv we can unit-test,
//! then run it. Running requires admin/root and is exercised manually.
use anyhow::{anyhow, Result};
use std::path::Path;

/// (program, args) to install `ca_cert` into the OS trust store.
pub fn install_command(ca_cert: &Path) -> (String, Vec<String>) {
    let p = ca_cert.display().to_string();
    #[cfg(windows)]
    {
        ("certutil".into(), vec!["-addstore".into(), "-f".into(), "Root".into(), p])
    }
    #[cfg(target_os = "macos")]
    {
        (
            "security".into(),
            vec![
                "add-trusted-cert".into(),
                "-d".into(),
                "-r".into(),
                "trustRoot".into(),
                "-k".into(),
                "/Library/Keychains/System.keychain".into(),
                p,
            ],
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Debian/Ubuntu convention: copy to the local CA dir, then update.
        ("cp".into(), vec![p, "/usr/local/share/ca-certificates/altkey.crt".into()])
    }
}

/// (program, args) to remove the altkey CA from the OS trust store.
pub fn uninstall_command() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        ("certutil".into(), vec!["-delstore".into(), "Root".into(), "altkey local CA".into()])
    }
    #[cfg(target_os = "macos")]
    {
        ("security".into(), vec!["delete-certificate".into(), "-c".into(), "altkey local CA".into()])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ("rm".into(), vec!["-f".into(), "/usr/local/share/ca-certificates/altkey.crt".into()])
    }
}

/// Run the install command. Returns Ok only on a zero exit code.
pub fn install(ca_cert: &Path) -> Result<()> {
    let (prog, args) = install_command(ca_cert);
    run(&prog, &args)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    run("update-ca-certificates", &[])?;
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let (prog, args) = uninstall_command();
    run(&prog, &args).ok(); // best-effort
    #[cfg(all(unix, not(target_os = "macos")))]
    run("update-ca-certificates", &["--fresh".to_string()]).ok();
    Ok(())
}

fn run(prog: &str, args: &[String]) -> Result<()> {
    let status = std::process::Command::new(prog)
        .args(args)
        .status()
        .map_err(|e| anyhow!("spawn {prog}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{prog} exited {:?}", status.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn install_command_references_the_cert_path() {
        let (prog, args) = install_command(&PathBuf::from("/tmp/altkey/ca.crt"));
        assert!(!prog.is_empty());
        assert!(
            args.iter().any(|a| a.contains("ca.crt")),
            "install argv must include the cert path"
        );
    }

    #[test]
    fn uninstall_command_is_nonempty() {
        let (prog, args) = uninstall_command();
        assert!(!prog.is_empty());
        assert!(!args.is_empty());
    }
}
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cd engine && cargo test transparent::trust`
Expected: PASS (2 tests). (These build the argv only; they never touch the trust store.)

- [ ] **Step 4: Commit**

```bash
git add engine/src/transparent/mod.rs engine/src/transparent/trust.rs
git commit -m "feat(engine): OS trust-store install/uninstall for the altkey CA"
```

---

## Task 6: The :443 intercept server (`transparent/server.rs`)

**Files:**
- Create: `engine/src/transparent/server.rs`
- Modify: `engine/src/transparent/mod.rs` (add `pub mod server;`)
- Create: `engine/tests/transparent_server.rs`

- [ ] **Step 1: Register the module**

Add to `engine/src/transparent/mod.rs`:

```rust
pub mod server;
```

- [ ] **Step 2: Implement the SNI TLS server that serves the existing router**

Create `engine/src/transparent/server.rs`:

```rust
//! HTTPS listener that terminates TLS for the intercepted hosts (SNI-selected
//! leaf certs) and serves the existing axum router. Because ALTKEY_TRANSPARENT
//! is set, the router accepts the tool's own key and uses the sub instead.
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
    ServerConfig,
};
use tokio_rustls::TlsAcceptor;

use crate::config;
use crate::transparent::ca::Ca;

/// Resolves a leaf cert per SNI hostname, minting + caching on first use.
#[derive(Debug)]
struct SniResolver {
    keys: parking_lot::Mutex<std::collections::HashMap<String, Arc<CertifiedKey>>>,
    ca: Arc<Ca>,
}

impl SniResolver {
    fn certified_for(&self, host: &str) -> Option<Arc<CertifiedKey>> {
        if let Some(k) = self.keys.lock().get(host) {
            return Some(k.clone());
        }
        let leaf = self.ca.mint_leaf(host).ok()?;
        let certs: Vec<CertificateDer<'static>> =
            rustls_pemfile::certs(&mut leaf.cert_pem.as_bytes())
                .filter_map(|c| c.ok())
                .collect();
        let key: PrivateKeyDer<'static> =
            rustls_pemfile::private_key(&mut leaf.key_pem.as_bytes()).ok()??;
        let signing = tokio_rustls::rustls::crypto::ring::sign::any_supported_type(&key).ok()?;
        let ck = Arc::new(CertifiedKey::new(certs, signing));
        self.keys.lock().insert(host.to_string(), ck.clone());
        Some(ck)
    }
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let host = hello.server_name()?;
        self.certified_for(host)
    }
}

/// Bind :443 and serve `app` (the existing router) over TLS, minting certs per SNI.
/// Returns the bound listener task; the caller keeps the handle to stop it.
pub async fn serve(app: axum::Router, ca: Arc<Ca>) -> Result<tokio::task::JoinHandle<()>> {
    let resolver = Arc::new(SniResolver {
        keys: parking_lot::Mutex::new(std::collections::HashMap::new()),
        ca,
    });
    let mut cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(cfg));
    let listener = TcpListener::bind(("127.0.0.1", 443))
        .await
        .context("bind :443 (needs admin/root)")?;

    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue; };
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else { return; };
                let svc = hyper::service::service_fn(move |req| {
                    let app = app.clone();
                    async move { app.oneshot(req).await }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(hyper_util::rt::TokioIo::new(tls), svc)
                .await;
            });
        }
    });
    Ok(handle)
}

/// Convenience: load-or-create the CA from config paths.
pub fn load_ca() -> Result<Arc<Ca>> {
    Ok(Arc::new(Ca::load_or_create(
        &config::ca_cert_path(),
        &config::ca_key_path(),
    )?))
}
```

Add to `engine/Cargo.toml` `[dependencies]` (needed by the server):

```toml
hyper-util = { version = "0.1", features = ["server", "tokio", "http1"] }
tower = { version = "0.5", features = ["util"] }
```

(`tower::ServiceExt::oneshot` drives the router per request; `hyper-util` bridges the TLS stream to the service.)

- [ ] **Step 3: Write the integration test**

Create `engine/tests/transparent_server.rs`:

```rust
//! Verifies the :443 intercept server terminates TLS with a CA-minted leaf and
//! routes an intercepted host into the router. Uses a loopback router (no real
//! provider call) so the test is hermetic.
use std::sync::Arc;

#[tokio::test]
async fn intercept_server_serves_router_over_tls() {
    // A tiny stand-in router that proves routing happened.
    let app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(|| async { "ok-transparent" }),
    );
    let ca = Arc::new(altkey::transparent::ca::Ca::generate().unwrap());

    // Bind an ephemeral port instead of 443 for the test by reusing serve logic
    // via a helper that accepts a listener. (See note below.)
    let handle = altkey::transparent::server::serve_for_test(app, ca.clone(), 0)
        .await
        .expect("serve");
    let port = handle.port;

    // Build a client that trusts our CA and connects to 127.0.0.1 but presents
    // SNI "api.openai.com".
    let mut roots = rustls::RootCertStore::empty();
    for c in rustls_pemfile::certs(&mut ca.cert_pem.as_bytes()) {
        roots.add(c.unwrap()).unwrap();
    }
    let body = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(ca.cert_pem.as_bytes()).unwrap())
        .resolve("api.openai.com", format!("127.0.0.1:{port}").parse().unwrap())
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
    handle.task.abort();
}
```

To make the test hermetic, add a test helper to `server.rs` that binds a given port and returns the port + task:

```rust
#[cfg(any(test, feature = "test-helpers"))]
pub struct TestServer {
    pub port: u16,
    pub task: tokio::task::JoinHandle<()>,
}

#[cfg(any(test, feature = "test-helpers"))]
pub async fn serve_for_test(
    app: axum::Router,
    ca: Arc<Ca>,
    port: u16,
) -> Result<TestServer> {
    let resolver = Arc::new(SniResolver {
        keys: parking_lot::Mutex::new(std::collections::HashMap::new()),
        ca,
    });
    let mut cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(cfg));
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let bound = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue; };
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else { return; };
                let svc = hyper::service::service_fn(move |req| {
                    let app = app.clone();
                    async move { app.clone().oneshot(req).await }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(hyper_util::rt::TokioIo::new(tls), svc)
                .await;
            });
        }
    });
    Ok(TestServer { port: bound, task })
}
```

Expose the crate as a lib for the integration test: ensure `engine/src/main.rs` has a sibling `engine/src/lib.rs` re-exporting the modules, or add `[lib]`/`[[bin]]` split. Add to `engine/Cargo.toml`:

```toml
[lib]
name = "altkey"
path = "src/lib.rs"
```

Create `engine/src/lib.rs`:

```rust
pub mod config;
pub mod transparent;
```

(Keep `main.rs` as the bin; it can `use altkey::...` or keep its own `mod` lines — for the test we only need `config` + `transparent` exposed via the lib.)

- [ ] **Step 4: Run the integration test**

Run: `cd engine && cargo test --test transparent_server`
Expected: PASS — the client trusting the altkey CA reaches the router over TLS and gets `ok-transparent`. If rustls/hyper-util API mismatches appear, align to the installed versions (rustls 0.23 / hyper-util 0.1) before moving on.

- [ ] **Step 5: Commit**

```bash
git add engine/Cargo.toml engine/Cargo.lock engine/src/lib.rs engine/src/transparent/mod.rs engine/src/transparent/server.rs engine/tests/transparent_server.rs
git commit -m "feat(engine): SNI TLS intercept server routing to the existing /v1 router"
```

---

## Task 7: Orchestrator + admin endpoints

**Files:**
- Modify: `engine/src/transparent/mod.rs` (add `enable`/`disable`/`status`)
- Modify: `engine/src/routes.rs` (add `/admin/transparent/{enable,disable,status}`)
- Modify: `engine/src/main.rs` (auto-enable if `ALTKEY_TRANSPARENT=1` on startup)

- [ ] **Step 1: Implement the orchestrator**

Replace the top of `engine/src/transparent/mod.rs` (keep the `pub mod` lines) so the file reads:

```rust
pub mod ca;
pub mod hosts;
pub mod server;
pub mod trust;

use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Turn transparent mode ON: ensure CA, trust it, redirect hosts, set the
/// per-request bypass, and start the :443 intercept server serving `app`.
/// Requires admin/root (hosts edit, trust install, :443 bind).
pub async fn enable(app: axum::Router) -> Result<()> {
    let ca = server::load_ca()?;
    trust::install(&crate::config::ca_cert_path())?;
    hosts::enable(&crate::config::hosts_path(), &crate::config::INTERCEPT_HOSTS)?;
    std::env::set_var("ALTKEY_TRANSPARENT", "1");
    let _task = server::serve(app, Arc::clone(&ca)).await?;
    ENABLED.store(true, Ordering::SeqCst);
    // Intentionally leak the task handle for the process lifetime; disable()
    // removes hosts + trust which is what users care about.
    std::mem::forget(_task);
    Ok(())
}

/// Turn transparent mode OFF: remove the hosts redirect + untrust the CA.
pub fn disable() -> Result<()> {
    hosts::disable(&crate::config::hosts_path())?;
    trust::uninstall()?;
    std::env::remove_var("ALTKEY_TRANSPARENT");
    ENABLED.store(false, Ordering::SeqCst);
    Ok(())
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}
```

- [ ] **Step 2: Add the admin routes**

In `engine/src/routes.rs`, in `build_router()`, add three routes (next to the other `/admin/*`):

```rust
        .route("/admin/transparent/enable", post(admin_transparent_enable))
        .route("/admin/transparent/disable", post(admin_transparent_disable))
        .route("/admin/transparent/status", get(admin_transparent_status))
```

And add the handlers at the bottom of `routes.rs`:

```rust
async fn admin_transparent_enable(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    // Rebuild the router to serve under transparent mode.
    let app = build_router();
    match crate::transparent::enable(app).await {
        Ok(()) => Json(json!({"ok": true, "transparent": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string(),
                "hint": "transparent mode needs admin/root for hosts + :443 + trust store"})),
        )
            .into_response(),
    }
}

async fn admin_transparent_disable(headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&headers) {
        return (e.0, e.1).into_response();
    }
    match crate::transparent::disable() {
        Ok(()) => Json(json!({"ok": true, "transparent": false})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}

async fn admin_transparent_status(headers: HeaderMap) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&headers)?;
    Ok(Json(json!({"transparent": crate::transparent::is_enabled()})))
}
```

Ensure `routes.rs` references `crate::transparent` — add `mod transparent;` to `main.rs` if not already present (Task 3 added it).

- [ ] **Step 3: Auto-enable on startup when requested**

In `engine/src/main.rs`, after the router is built and before `axum::serve`, add:

```rust
    if std::env::var("ALTKEY_TRANSPARENT").as_deref() == Ok("1") {
        let app_for_transparent = routes::build_router();
        match transparent::enable(app_for_transparent).await {
            Ok(()) => tracing::info!("transparent mode ON (api.openai.com/api.anthropic.com intercepted)"),
            Err(e) => tracing::warn!("transparent mode failed: {e} (need admin/root?)"),
        }
    }
```

- [ ] **Step 4: Build + run existing tests**

Run: `cd engine && cargo build && cargo test`
Expected: compiles; all prior unit + integration tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add engine/src/transparent/mod.rs engine/src/routes.rs engine/src/main.rs
git commit -m "feat(engine): transparent-mode orchestrator + /admin/transparent endpoints"
```

---

## Task 8: End-to-end manual verification (real machine, admin)

**Files:** none (manual). Requires an **administrator** shell because it edits the hosts file, installs a trust cert, and binds :443.

- [ ] **Step 1: Confirm a provider is connected**

Run (normal shell): `curl -s http://127.0.0.1:8787/v1/models -H "Authorization: Bearer <a-minted-sk-alt-key>" | head`
Expected: a models list (engine running, sub connected).

- [ ] **Step 2: Enable transparent mode (admin shell)**

Start the engine from an **elevated** terminal with `ALTKEY_TRANSPARENT=1`, OR call the admin endpoint:

PowerShell (Run as Administrator):
```powershell
$env:ALTKEY_TRANSPARENT="1"; .\engine\target\release\altkey.exe
```
Expected log: `transparent mode ON (api.openai.com/api.anthropic.com intercepted)`.

- [ ] **Step 3: Verify the redirect + cert took effect**

Run: `ping api.openai.com` → resolves to `127.0.0.1`.
Run:
```bash
curl -s https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer sk-anything-the-tool-has" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"say transparent works"}]}'
```
Expected: a real completion ("transparent works…") served from your ChatGPT sub, **with no base-URL set and a junk key** — proving the intercept + transparent bypass + provider call all work. (`curl` trusts the system store, where the altkey CA was installed.)

- [ ] **Step 4: Verify a real hardcoded-URL tool works with zero config**

Point gstack's `design.exe` at altkey **without** `OPENAI_BASE_URL` (transparent mode handles it):
```bash
OPENAI_API_KEY=sk-anything ~/.claude/skills/gstack/design/dist/design.exe generate \
  --brief "a tiny red dot, minimal" --output /tmp/transparent-test.png
```
Expected: an image generates — the tool's hardcoded `api.openai.com` was transparently intercepted, **no patch, no base URL.** This is the headline result of the whole plan.

- [ ] **Step 5: Disable + confirm clean teardown**

Call: `curl -s -X POST http://127.0.0.1:8787/admin/transparent/disable`
Run: `ping api.openai.com` → resolves to the **real** OpenAI again (hosts block removed).
Confirm the altkey CA can be removed from the trust store (status shows `transparent: false`).

- [ ] **Step 6: Commit a short verification note**

```bash
git commit --allow-empty -m "test(engine): transparent mode verified end-to-end (gstack zero-config)"
```

---

## Self-Review

**Spec coverage:** This plan implements the spec's "transparent mode" component (agent intercepts `api.openai.com`/`api.anthropic.com` locally via CA + hosts redirect, serves the existing router, zero-config, removes per-tool patching), the security note (CA install is real + disclosed, machine-scoped), and the "Usage modes → Transparent (local)" row. The tunnel, relay, control plane, billing, and desktop app are **out of scope for this plan** (their own plans, per the decomposition).

**Placeholder scan:** No "TBD/TODO/handle edge cases" — every step has concrete code or commands. The only deferred items are explicit cross-version API alignments (rcgen 0.13 / rustls 0.23 / hyper-util 0.1), called out where they occur.

**Type consistency:** `Ca`, `Leaf`, `Ca::generate/mint_leaf/load_or_create/save`, `hosts::{apply_block,strip_block,enable,disable}`, `trust::{install_command,uninstall_command,install,uninstall}`, `server::{serve,serve_for_test,load_ca}`, `transparent::{enable,disable,is_enabled}` are used consistently across tasks. `config::{INTERCEPT_HOSTS,ca_cert_path,ca_key_path,hosts_path}` are defined in Task 2 and referenced thereafter.

**Known risk to watch during execution:** exact crate API surfaces (rcgen leaf signing, rustls `CertifiedKey`/`ResolvesServerCert`, hyper-util serve glue) drift across minor versions — the first compile of Tasks 3, 5 is where to reconcile. The TLS-passthrough work (relay never decrypts) lives in the **tunnel** plan, not here; this plan terminates TLS locally on the user's own machine, which is correct for transparent mode.
