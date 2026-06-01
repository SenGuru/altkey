# altkey Plan 4 — Desktop App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A small Tauri 2 desktop app — the reduced local front door to the altkey agent. It shows this machine's status (agent running? tunnel up? reachable URL), lets the user start/stop the agent + tunnel, connect providers, and see/mint this-machine keys — talking to the local agent over `127.0.0.1:8787` and deep-linking to the web app for account/billing.

**Architecture:** A `desktop/` Tauri 2 project: a Rust `src-tauri` crate exposing `#[tauri::command]`s that bridge to the local agent's HTTP admin API (status, tunnel start/stop) and manage the agent process (spawn/stop the `altkey` binary); a Vite/React/TS frontend (reusing the warm-dark/brass design + patterns from `web/`) with Status / Providers / Tunnel / Keys views. Verification is compile-based: `cargo build` on `src-tauri` compiles and the frontend `bun run build` succeeds. Native bundling (`tauri build`) + GUI QA are deploy-time (need a display / signing), out of automated scope here — same posture as the backend's deploy-time DNS/ACME/credentials.

**Tech Stack:** Tauri 2, Rust (reqwest to the local agent, std::process for lifecycle), Vite 5 + React 18 + TS, bun. WebView2 (present on Win11).

**Branch:** `feat/desktop-app` off `dev`.

**Out of scope (web app owns these):** account login, billing/buy, cross-machine views, the full validation authority. The desktop app is local-only + deep-links out.

---

## File Structure

| File | Responsibility |
|---|---|
| `desktop/package.json`, `vite.config.ts`, `tsconfig.json`, `index.html` | frontend project |
| `desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, `build.rs` | Tauri Rust crate config |
| `desktop/src-tauri/src/main.rs` | Tauri app entry + command registration |
| `desktop/src-tauri/src/agent.rs` | bridge to local agent HTTP + process lifecycle |
| `desktop/src/lib/bridge.ts` | typed wrappers around `invoke(...)` |
| `desktop/src/pages/{Status,Providers,Tunnel,Keys}.tsx` | views |
| `desktop/src/styles/theme.css` | reuse warm-dark/brass tokens |

**Bridge command contract (Rust ↔ TS):**
```rust
#[tauri::command] async fn agent_status() -> AgentStatus // { running, tunnel_up, handle, reachable_url }
#[tauri::command] async fn start_agent() -> Result<(), String>
#[tauri::command] async fn stop_agent() -> Result<(), String>
#[tauri::command] async fn start_tunnel() -> Result<(), String>   // POST /admin/tunnel/start
#[tauri::command] async fn stop_tunnel() -> Result<(), String>    // POST /admin/tunnel/stop
#[tauri::command] async fn open_web(path: String)                 // open the web app in the browser
```

---

## Task 1: Tauri scaffold (compiles + frontend builds)

**Files:** Create the `desktop/` project skeleton.

- [ ] **Step 1: Frontend skeleton** — create `desktop/package.json`:
```json
{
  "name": "altkey-desktop",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "typecheck": "tsc --noEmit",
    "build": "tsc --noEmit && vite build",
    "tauri": "tauri"
  },
  "dependencies": { "react": "^18.3.1", "react-dom": "^18.3.1" },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@tauri-apps/api": "^2.0.0",
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0"
  }
}
```
`desktop/vite.config.ts` (react plugin; `clearScreen: false`; `server.port: 1420`, `server.strictPort: true` — Tauri convention), `desktop/tsconfig.json` (strict React), `desktop/index.html`, `desktop/src/main.tsx` (render App), `desktop/src/App.tsx` (placeholder "altkey desktop"), `desktop/.gitignore` (node_modules, dist, src-tauri/target).

- [ ] **Step 2: Tauri Rust crate** — create `desktop/src-tauri/Cargo.toml`:
```toml
[package]
name = "altkey-desktop"
version = "0.1.0"
edition = "2021"

[lib]
name = "altkey_desktop_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
tokio = { version = "1", features = ["rt-multi-thread", "process", "macros"] }
```
NOTE: This crate is NOT added to the root cargo workspace (Tauri apps manage their own target). Add `desktop/src-tauri` to the workspace `exclude` list in the root `Cargo.toml` if cargo tries to absorb it, OR keep it standalone (it has its own Cargo.lock). RECOMMENDED: add `exclude = ["desktop/src-tauri"]` to the `[workspace]` table so the main workspace build ignores it.

`desktop/src-tauri/build.rs`: `fn main() { tauri_build::build() }`.
`desktop/src-tauri/tauri.conf.json`: a minimal Tauri 2 config (productName "altkey", identifier "app.altkey.desktop", build.frontendDist "../dist", build.devUrl "http://localhost:1420", a single window 1000x700, `app.security.csp` null for now). Reconcile to the Tauri 2 schema.
`desktop/src-tauri/src/main.rs` (minimal, no commands yet):
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Install + verify compile + build**

```bash
cd "C:/Users/gsent/Desktop/altkey/desktop" && bun install && bun run build
cd "C:/Users/gsent/Desktop/altkey/desktop/src-tauri" && cargo build
```
Expected: `bun run build` produces `desktop/dist`; `cargo build` compiles the src-tauri crate (downloads tauri 2 — first build is slow; that's fine). If `tauri::generate_context!` requires `tauri.conf.json` fields, reconcile to the Tauri 2 config schema until it compiles. If a frontend dist is required at compile time, ensure `bun run build` ran first.
- Confirm the MAIN workspace still builds: `cd "C:/Users/gsent/Desktop/altkey" && cargo build --workspace` (the excluded desktop crate shouldn't be pulled in).

- [ ] **Step 4: Commit**

```bash
git checkout -b feat/desktop-app
git add desktop Cargo.toml
git commit -m "feat(desktop): Tauri 2 scaffold (compiles + frontend builds)"
```

---

## Task 2: Agent bridge — status + tunnel control + lifecycle

**Files:** Create `desktop/src-tauri/src/agent.rs`; modify `src-tauri/src/main.rs`.

- [ ] **Step 1: Bridge module** — create `desktop/src-tauri/src/agent.rs`:
```rust
//! Bridge to the local altkey agent: read status + toggle the tunnel over its
//! 127.0.0.1:8787 admin API, and manage the agent process lifecycle. In local
//! mode the agent's admin endpoints require no token.
use serde::Serialize;

const AGENT_BASE: &str = "http://127.0.0.1:8787";

#[derive(Serialize, Default)]
pub struct AgentStatus {
    pub running: bool,
    pub tunnel_up: bool,
    pub handle: Option<String>,
    pub reachable_url: Option<String>,
}

async fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(3)).build().unwrap()
}

#[tauri::command]
pub async fn agent_status() -> AgentStatus {
    let client = http().await;
    // Is the agent up? Probe a cheap endpoint.
    let running = client.get(format!("{AGENT_BASE}/v1/models")).send().await.is_ok();
    if !running {
        return AgentStatus::default();
    }
    // Tunnel status (admin endpoint; open in local mode).
    let mut status = AgentStatus { running: true, ..Default::default() };
    if let Ok(resp) = client.get(format!("{AGENT_BASE}/admin/tunnel/status")).send().await {
        if let Ok(v) = resp.json::<serde_json::Value>().await {
            status.tunnel_up = v.get("tunnel_up").and_then(|x| x.as_bool()).unwrap_or(false);
            status.handle = v.get("handle").and_then(|x| x.as_str()).map(String::from);
            status.reachable_url = status.handle.as_ref().map(|h| format!("https://{h}.altkey.app/v1"));
        }
    }
    status
}

#[tauri::command]
pub async fn start_tunnel() -> Result<(), String> {
    http().await.post(format!("{AGENT_BASE}/admin/tunnel/start")).send().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn stop_tunnel() -> Result<(), String> {
    http().await.post(format!("{AGENT_BASE}/admin/tunnel/stop")).send().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Spawn the agent binary if it isn't already running. The binary path comes from
/// ALTKEY_AGENT_BIN (set at install time) or defaults to "altkey" on PATH.
#[tauri::command]
pub async fn start_agent() -> Result<(), String> {
    if agent_status().await.running {
        return Ok(());
    }
    let bin = std::env::var("ALTKEY_AGENT_BIN").unwrap_or_else(|_| "altkey".into());
    tokio::process::Command::new(bin)
        .spawn()
        .map_err(|e| format!("failed to start agent: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn open_web(path: String) -> Result<(), String> {
    let base = std::env::var("ALTKEY_WEB_URL").unwrap_or_else(|_| "https://altkey.app".into());
    let url = format!("{base}{path}");
    // Open in the default browser.
    open_url(&url).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn().map(|_| ())
}
#[cfg(not(target_os = "windows"))]
fn open_url(url: &str) -> std::io::Result<()> {
    let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(cmd).arg(url).spawn().map(|_| ())
}
```
NOTE: `stop_agent` (graceful) is harder cross-platform (need the PID). For MVP, omit a hard stop or implement a best-effort: track the spawned child in Tauri state. KEEP IT SIMPLE — if tracking the child is awkward, expose `stop_agent` that returns an informative error ("stop the agent from its own console for now") OR store the `Child` in a `tauri::State<Mutex<Option<Child>>>` populated by `start_agent` and kill it. Pick the simpler path that compiles; document the choice.

- [ ] **Step 2: Register commands** — `src-tauri/src/main.rs`:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod agent;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            agent::agent_status,
            agent::start_tunnel,
            agent::stop_tunnel,
            agent::start_agent,
            agent::open_web,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Verify compile** — `cd desktop/src-tauri && cargo build` compiles with the commands. Commit:
```bash
git add desktop/src-tauri/src
git commit -m "feat(desktop): agent bridge commands (status, tunnel, lifecycle, open-web)"
```

---

## Task 3: Frontend — Status / Tunnel / Providers / Keys

**Files:** Create `desktop/src/lib/bridge.ts`, `desktop/src/pages/*`, `desktop/src/styles/theme.css`; modify `App.tsx`.

- [ ] **Step 1: Typed bridge** — `desktop/src/lib/bridge.ts`:
```ts
import { invoke } from '@tauri-apps/api/core';
export interface AgentStatus { running: boolean; tunnel_up: boolean; handle: string | null; reachable_url: string | null; }
export const agentStatus = () => invoke<AgentStatus>('agent_status');
export const startTunnel = () => invoke<void>('start_tunnel');
export const stopTunnel = () => invoke<void>('stop_tunnel');
export const startAgent = () => invoke<void>('start_agent');
export const openWeb = (path: string) => invoke<void>('open_web', { path });
```

- [ ] **Step 2: App + pages** — `App.tsx`: a simple tab/section layout. `pages/Status.tsx`: polls `agentStatus()` (every 3s), shows agent running (green/red), tunnel up, the reachable URL with a copy button, and Start-agent / Start-tunnel / Stop-tunnel buttons wired to the bridge. `pages/Providers.tsx`: "Connect a provider" buttons that `openWeb('/providers')` (the agent's provider OAuth is CLI/local — for MVP, deep-link to web instructions OR show the local connect steps; keep it informational + a link). `pages/Keys.tsx`: deep-link `openWeb('/keys')` to mint keys in the web dashboard (the desktop app shows this-machine status; key minting stays in the web authority) + display the reachable URL to use as the base URL. `pages/Tunnel.tsx` can merge into Status (tunnel toggle). Reuse the warm-dark/brass `theme.css` (copy from `web/src/styles/theme.css`).

- [ ] **Step 3: Verify + commit** — `cd desktop && bun run build` succeeds (tsc + vite). Commit:
```bash
git add desktop/src
git commit -m "feat(desktop): status/providers/keys UI wired to the agent bridge"
```

---

## Task 4: Final verification

- [ ] **Step 1:** `cd "C:/Users/gsent/Desktop/altkey/desktop" && bun run build` (frontend builds) AND `cd src-tauri && cargo build` (Rust compiles).
- [ ] **Step 2:** `cd "C:/Users/gsent/Desktop/altkey" && cargo build --workspace && cargo test --workspace` — the MAIN workspace is unaffected by the desktop crate (it's excluded) and all backend tests stay green.
- [ ] **Step 3:** Document in the commit that native bundling (`bun run tauri build`) + GUI QA are deploy-time (require a display / code signing), out of automated scope. Commit any fixups:
```bash
git add -A
git commit -m "chore(desktop): final compile + frontend build verification"
```
(Skip if nothing.)

---

## Self-Review

**Spec coverage (Plan 4 slice):** Implements the spec's "altkey-desktop — the reduced local GUI": this-machine status (agent running, tunnel on/off, reachable URL), start/stop agent + tunnel (via the agent's local admin API), provider connect + key management deep-linked to the web authority, talking to the agent over `127.0.0.1:8787` and out to the web app — exactly the "reduced, links out for billing/cross-machine" split from the spec's feature table. Out of scope (web owns): login, billing, validation authority, cross-machine.

**Placeholder scan:** The two reconciliation points (Tauri 2 `tauri.conf.json` schema in Task 1; `stop_agent` child-tracking vs informative-error in Task 2) specify the required OUTCOME and a concrete simpler fallback — real adaptation to Tauri 2, not gaps. Provider-connect is intentionally a deep-link for MVP (the agent's provider OAuth is a local/CLI flow; the desktop click-through is a future refinement) — a stated scope decision.

**Type consistency:** the `AgentStatus` shape is identical in `agent.rs` (Rust Serialize) and `bridge.ts` (TS interface); command names match between `generate_handler!` and the `invoke('...')` calls. The desktop crate is excluded from the cargo workspace so it can't break the backend build.

**Verification posture:** compile-based (src-tauri `cargo build` + frontend `bun run build`) — appropriate, since GUI behavior and native bundling need a display/signing and are deploy-time (mirrors the backend's deploy-time DNS/ACME/OAuth-credentials). The main workspace + all backend tests remain green (the desktop crate is excluded).
