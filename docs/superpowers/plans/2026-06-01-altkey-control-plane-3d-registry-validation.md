# altkey Control Plane 3d — Registry & Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make altkey actually function as a product: users claim a `<handle>`, pair a machine (mint an `ak_agent_` token) and mint `ak_live_` endpoint keys in the dashboard; the control plane exposes the **internal validation API**; and the **relay's `validate()` stub** + the **engine's local-only `require_key`** are rewired to that authority (with caching + offline grace). This is the survival-critical slice — after it, a request through `<handle>.altkey.app` is gated by a real account + active subscription.

**Architecture:** Adds `handle`, `agent`, `endpoint_key` tables + registry CRUD routes (session-gated, mint-once secrets). Adds the internal endpoints `/internal/agent/authorize`, `/internal/key/validate`, `/internal/agent/heartbeat` — authenticated by the relay's service secret and/or a valid agent token, using **constant-time** hash comparison. Shared request/response DTOs live in `altkey-api` so the relay (Rust) and engine (Rust) call typed contracts. The relay's `validate()` becomes an async HTTP call to `authorize`; the engine gains a `KeyValidator` that checks `ak_live_` against `key/validate` with a short cache + bounded offline grace, falling back to the existing local store when the control plane is not configured (keeps all existing tests green).

**Tech Stack:** Rust, axum 0.7, SeaORM 1.1.20, reqwest, `subtle` (constant-time compare), the `altkey-api` contract crate, the license gate from 3c.

**Branch:** `feat/control-plane-3d` off `dev`.

---

## File Structure

| File | Responsibility |
|---|---|
| `altkey-api/src/token.rs` | add `verify_hash` (constant-time) (modify) |
| `altkey-api/src/dto.rs` + `lib.rs` | shared internal-API DTOs (new) |
| `altkey-api/Cargo.toml` | add `subtle` (modify) |
| `control-plane/migration/src/m20260601_000004_create_registry.rs` | handle/agent/endpoint_key tables |
| `control-plane/migration/src/lib.rs` | register migration #4 (modify) |
| `control-plane/src/entities/{handle,agent,endpoint_key}.rs` + mod/prelude | entities (modify mod/prelude) |
| `control-plane/src/registry/mod.rs` | module root |
| `control-plane/src/registry/store.rs` | claim handle, pair agent, mint key, lookups |
| `control-plane/src/registry/routes.rs` | session-gated handle/agent/key CRUD |
| `control-plane/src/internal/mod.rs` | module root |
| `control-plane/src/internal/auth.rs` | service-secret + agent-token guards |
| `control-plane/src/internal/routes.rs` | authorize / key-validate / heartbeat |
| `control-plane/src/app.rs` | register registry + internal routes (modify) |
| `control-plane/src/lib.rs` | `pub mod registry; pub mod internal;` (modify) |
| `relay/src/agent_conn.rs` | `validate()` → async HTTP authorize (modify) |
| `relay/src/config.rs` (new) or inline | `CONTROL_PLANE_URL` + `INTERNAL_SERVICE_SECRET` |
| `relay/Cargo.toml` | add `reqwest`, `altkey-api` as deps (modify) |
| `engine/src/license.rs` | `KeyValidator` trait + `ControlPlaneValidator` (cache+grace) (new) |
| `engine/src/auth.rs` | `require_key` consults the validator when configured (modify) |
| `engine/Cargo.toml` | add `altkey-api` dep (modify) |
| tests | registry CRUD, internal truth-table, relay-authorize integration, engine key-validate |

**Shared DTOs (altkey-api/src/dto.rs) — relay + engine + control-plane:**
```rust
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AuthorizeRequest { pub handle: String, pub agent_token: String }
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Limits { pub max_concurrency: u32, pub max_rps: u32 } // 0 = unlimited (pro)
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AuthorizeResponse { pub ok: bool, pub account_id: String, pub plan: String, pub limits: Limits }
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct KeyValidateRequest { pub key: String, pub agent_token: String }
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct KeyValidateResponse { pub valid: bool, pub sub_active: bool, pub plan: String }
```

---

## Task 1: Constant-time hash compare + shared DTOs (`altkey-api`)

**Files:**
- Modify: `altkey-api/Cargo.toml`, `altkey-api/src/token.rs`, `altkey-api/src/lib.rs`
- Create: `altkey-api/src/dto.rs`
- Test: in-file unit tests

- [ ] **Step 1: Add `subtle`**

`altkey-api/Cargo.toml` `[dependencies]`: add `subtle = "2"`.

- [ ] **Step 2: Constant-time verify in token.rs**

Append to `altkey-api/src/token.rs`:
```rust
use subtle::ConstantTimeEq;

/// Constant-time check that `plaintext` hashes to `stored_hash` (both sha256 hex).
/// Avoids a timing oracle when validating presented tokens/keys against stored hashes.
pub fn verify_hash(plaintext: &str, stored_hash: &str) -> bool {
    let computed = hash(plaintext);
    let a = computed.as_bytes();
    let b = stored_hash.as_bytes();
    a.len() == b.len() && a.ct_eq(b).into()
}
```
Add a test:
```rust
    #[test]
    fn verify_hash_matches_only_correct_plaintext() {
        let t = generate(TokenKind::Live);
        let h = hash(&t.secret);
        assert!(verify_hash(&t.secret, &h));
        assert!(!verify_hash("wrong", &h));
    }
```

- [ ] **Step 3: DTOs**

Create `altkey-api/src/dto.rs` with the six types from the "Shared DTOs" block above (AuthorizeRequest, Limits, AuthorizeResponse, KeyValidateRequest, KeyValidateResponse). Add `pub mod dto;` to `altkey-api/src/lib.rs`.

- [ ] **Step 4: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p altkey-api`
Expected: token tests (incl. verify_hash) PASS.
```bash
git checkout -b feat/control-plane-3d
git add altkey-api
git commit -m "feat(altkey-api): constant-time verify_hash + internal-API DTOs"
```

---

## Task 2: Registry tables + entities

**Files:**
- Create: `control-plane/migration/src/m20260601_000004_create_registry.rs`; modify `migration/src/lib.rs`
- Create: `control-plane/src/entities/{handle,agent,endpoint_key}.rs`; modify `entities/mod.rs`, `prelude.rs`
- Test: `control-plane/tests/registry_entities.rs`

- [ ] **Step 1: Migration**

Create `control-plane/migration/src/m20260601_000004_create_registry.rs`:
```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Handle { Table, Id, AccountId, Name, Status, CreatedAt }
#[derive(DeriveIden)]
enum Agent { Table, Id, AccountId, HandleId, Name, AgentTokenHash, TokenPrefix, Status, CreatedAt, LastSeenAt }
#[derive(DeriveIden)]
enum EndpointKey { Table, Id, AccountId, AgentId, KeyHash, KeyPrefix, Name, CreatedAt, LastUsedAt, RevokedAt }

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(Table::create().table(Handle::Table).if_not_exists()
            .col(ColumnDef::new(Handle::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(Handle::AccountId).uuid().not_null())
            .col(ColumnDef::new(Handle::Name).string().not_null())
            .col(ColumnDef::new(Handle::Status).string().not_null().default("active"))
            .col(ColumnDef::new(Handle::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .to_owned()).await?;
        manager.create_index(Index::create().name("idx_handle_name_unique").table(Handle::Table).col(Handle::Name).unique().to_owned()).await?;

        manager.create_table(Table::create().table(Agent::Table).if_not_exists()
            .col(ColumnDef::new(Agent::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(Agent::AccountId).uuid().not_null())
            .col(ColumnDef::new(Agent::HandleId).uuid().not_null())
            .col(ColumnDef::new(Agent::Name).string().not_null())
            .col(ColumnDef::new(Agent::AgentTokenHash).string().not_null())
            .col(ColumnDef::new(Agent::TokenPrefix).string().not_null())
            .col(ColumnDef::new(Agent::Status).string().not_null().default("active"))
            .col(ColumnDef::new(Agent::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .col(ColumnDef::new(Agent::LastSeenAt).timestamp_with_time_zone().null())
            .to_owned()).await?;
        manager.create_index(Index::create().name("idx_agent_token_hash_unique").table(Agent::Table).col(Agent::AgentTokenHash).unique().to_owned()).await?;

        manager.create_table(Table::create().table(EndpointKey::Table).if_not_exists()
            .col(ColumnDef::new(EndpointKey::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(EndpointKey::AccountId).uuid().not_null())
            .col(ColumnDef::new(EndpointKey::AgentId).uuid().null())
            .col(ColumnDef::new(EndpointKey::KeyHash).string().not_null())
            .col(ColumnDef::new(EndpointKey::KeyPrefix).string().not_null())
            .col(ColumnDef::new(EndpointKey::Name).string().not_null())
            .col(ColumnDef::new(EndpointKey::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .col(ColumnDef::new(EndpointKey::LastUsedAt).timestamp_with_time_zone().null())
            .col(ColumnDef::new(EndpointKey::RevokedAt).timestamp_with_time_zone().null())
            .to_owned()).await?;
        manager.create_index(Index::create().name("idx_endpoint_key_hash_unique").table(EndpointKey::Table).col(EndpointKey::KeyHash).unique().to_owned()).await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(EndpointKey::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Agent::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Handle::Table).to_owned()).await
    }
}
```
Register as migration #4 in `migration/src/lib.rs`.

- [ ] **Step 2: Entities**

Create `control-plane/src/entities/handle.rs`, `agent.rs`, `endpoint_key.rs` matching the columns (pattern-match `account.rs`/`subscription.rs`). Types: UUIDs, `Option<Uuid>` for `endpoint_key.agent_id`, `Option<DateTimeWithTimeZone>` for nullable timestamps, `String` for the rest. Add to `entities/mod.rs` + `prelude.rs` (`Handle`, `Agent`, `EndpointKey`).

- [ ] **Step 3: Test + commit**

Create `control-plane/tests/registry_entities.rs` that migrates SQLite and round-trips one row in each table (pattern from `auth_entities.rs`/`billing_entity.rs`).
Run: `cargo test -p control-plane --test registry_entities` → PASS.
```bash
git add control-plane/migration control-plane/src/entities control-plane/tests/registry_entities.rs
git commit -m "feat(control-plane): handle/agent/endpoint_key tables + entities"
```

---

## Task 3: Registry store + session-gated routes

**Files:**
- Create: `control-plane/src/registry/mod.rs`, `store.rs`, `routes.rs`
- Modify: `control-plane/src/lib.rs`, `app.rs`
- Test: `control-plane/tests/registry_routes.rs`

- [ ] **Step 1: Store**

Create `control-plane/src/registry/mod.rs`:
```rust
pub mod routes;
pub mod store;
```
Create `control-plane/src/registry/store.rs` with (full impls — pattern-match earlier stores):
```rust
//! Registry writes/reads: claim a handle, pair an agent (mint ak_agent_), mint an
//! ak_live_ key. Secrets are returned in plaintext ONCE; only the hash is stored.
use crate::entities::{agent, endpoint_key, handle, prelude::*};
use altkey_api::token::{self, TokenKind};
use anyhow::{anyhow, Result};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

pub fn valid_handle_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.len() <= 63
        && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !n.starts_with('-') && !n.ends_with('-')
}

pub async fn handle_available(db: &DatabaseConnection, name: &str) -> Result<bool> {
    Ok(Handle::find().filter(handle::Column::Name.eq(name)).one(db).await?.is_none())
}

pub async fn claim_handle(db: &DatabaseConnection, account_id: Uuid, name: &str) -> Result<handle::Model> {
    let name = name.trim().to_lowercase();
    if !valid_handle_name(&name) { return Err(anyhow!("invalid handle name")); }
    if !handle_available(db, &name).await? { return Err(anyhow!("handle taken")); }
    Ok(handle::ActiveModel {
        id: Set(Uuid::new_v4()), account_id: Set(account_id), name: Set(name),
        status: Set("active".into()), created_at: Set(Utc::now().into()),
    }.insert(db).await?)
}

pub struct PairedAgent { pub agent: agent::Model, pub token_plaintext: String }

pub async fn pair_agent(db: &DatabaseConnection, account_id: Uuid, handle_id: Uuid, name: &str) -> Result<PairedAgent> {
    // Confirm the handle belongs to the account.
    let h = Handle::find_by_id(handle_id).one(db).await?.ok_or_else(|| anyhow!("no such handle"))?;
    if h.account_id != account_id { return Err(anyhow!("handle not owned")); }
    let tok = token::generate(TokenKind::Agent);
    let model = agent::ActiveModel {
        id: Set(Uuid::new_v4()), account_id: Set(account_id), handle_id: Set(handle_id),
        name: Set(name.to_string()), agent_token_hash: Set(token::hash(&tok.secret)),
        token_prefix: Set(token::prefix(&tok.secret)), status: Set("active".into()),
        created_at: Set(Utc::now().into()), last_seen_at: Set(None),
    }.insert(db).await?;
    Ok(PairedAgent { agent: model, token_plaintext: tok.secret })
}

pub struct MintedKey { pub key: endpoint_key::Model, pub key_plaintext: String }

pub async fn mint_key(db: &DatabaseConnection, account_id: Uuid, agent_id: Option<Uuid>, name: &str) -> Result<MintedKey> {
    let tok = token::generate(TokenKind::Live);
    let model = endpoint_key::ActiveModel {
        id: Set(Uuid::new_v4()), account_id: Set(account_id), agent_id: Set(agent_id),
        key_hash: Set(token::hash(&tok.secret)), key_prefix: Set(token::prefix(&tok.secret)),
        name: Set(name.to_string()), created_at: Set(Utc::now().into()),
        last_used_at: Set(None), revoked_at: Set(None),
    }.insert(db).await?;
    Ok(MintedKey { key: model, key_plaintext: tok.secret })
}
```

- [ ] **Step 2: Routes**

Create `control-plane/src/registry/routes.rs` with session-gated handlers (all take `CurrentAccount`): `list_handles` (GET /handles), `handle_availability` (GET /handles/availability?name=), `create_handle` (POST /handles {name}), `delete_handle` (DELETE /handles/{id} → set status revoked, verify ownership); `list_agents` (GET /agents), `create_agent` (POST /agents {handle_id,name} → returns `{agent, token}` with the plaintext ONCE), `delete_agent` (DELETE /agents/{id} → revoke); `list_keys` (GET /keys), `create_key` (POST /keys {name, agent_id?} → returns `{key, secret}` ONCE), `delete_key` (DELETE /keys/{id} → set revoked_at). Each verifies the row's `account_id == acct.id` before mutating. Use `#[utoipa::path]` on each + `ToSchema` response structs (e.g. `CreatedAgent { id, name, handle_id, token }`, `CreatedKey { id, name, prefix, secret }`, list views WITHOUT the secret). Mint endpoints return the secret; list endpoints return only `prefix`.

- [ ] **Step 3: Register + lib**

`pub mod registry;` in lib.rs. Register all routes in `app.rs` via `.routes(routes!(...))`.

- [ ] **Step 4: Test**

Create `control-plane/tests/registry_routes.rs`: boot app, seed account + session, then: claim a handle (assert availability flips), pair an agent (assert the response includes an `ak_agent_` token + listing shows only the prefix), mint a key (assert `ak_live_` secret returned once + listing hides it), delete a key (assert it's gone/revoked). Assert `/handles` is in the served openapi.json.

- [ ] **Step 5: Run + commit**

Run: `cargo test -p control-plane --test registry_routes` → PASS.
```bash
git add control-plane/src/registry control-plane/src/lib.rs control-plane/src/app.rs control-plane/tests/registry_routes.rs
git commit -m "feat(control-plane): registry store + handle/agent/key routes (mint-once secrets)"
```

---

## Task 4: Internal validation endpoints

**Files:**
- Create: `control-plane/src/internal/mod.rs`, `auth.rs`, `routes.rs`
- Modify: `control-plane/src/lib.rs`, `app.rs`
- Test: `control-plane/tests/internal_validate.rs`

- [ ] **Step 1: Internal guards**

Create `control-plane/src/internal/mod.rs`:
```rust
pub mod auth;
pub mod routes;
```
Create `control-plane/src/internal/auth.rs`:
```rust
//! Guards for the agent/relay-facing endpoints. The relay presents the service
//! secret (header). The agent presents its agent token (in the request body), which
//! is resolved to an account via a constant-time hash lookup.
use crate::entities::{agent, prelude::Agent};
use crate::state::AppState;
use altkey_api::token;
use axum::http::HeaderMap;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

/// True if the request carries the configured internal service secret.
pub fn service_secret_ok(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.config.internal_service_secret.as_ref() else { return false };
    let supplied = headers.get("x-altkey-service-secret").and_then(|v| v.to_str().ok()).unwrap_or("");
    // constant-time-ish: compare equal-length byte slices
    use subtle::ConstantTimeEq;
    let a = supplied.as_bytes();
    let b = expected.as_bytes();
    a.len() == b.len() && a.ct_eq(b).into()
}

/// Resolve an agent token to its (active) agent row, constant-time on the hash.
pub async fn agent_for_token(state: &AppState, agent_token: &str) -> Option<agent::Model> {
    let hash = token::hash(agent_token);
    let candidate = Agent::find().filter(agent::Column::AgentTokenHash.eq(hash)).one(&state.db).await.ok()??;
    if candidate.status != "active" { return None; }
    // The DB lookup is by exact hash; re-verify constant-time for defense in depth.
    if token::verify_hash(agent_token, &candidate.agent_token_hash) { Some(candidate) } else { None }
}
```

- [ ] **Step 2: Internal routes**

Create `control-plane/src/internal/routes.rs`:
```rust
//! The validation authority. `authorize` (relay) confirms a tunnel: agent token
//! valid + owns the handle + sub active → plan + limits. `key_validate` (agent)
//! confirms an ak_live_ key + the account's sub. `heartbeat` updates last_seen.
use crate::billing::store::active_subscription;
use crate::entities::{agent, endpoint_key, handle, prelude::*};
use crate::internal::auth::{agent_for_token, service_secret_ok};
use crate::state::AppState;
use altkey_api::dto::{AuthorizeRequest, AuthorizeResponse, KeyValidateRequest, KeyValidateResponse, Limits};
use altkey_api::token;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

fn limits_for(plan: &str) -> Limits {
    match plan {
        "pro" => Limits { max_concurrency: 0, max_rps: 0 }, // unlimited
        _ => Limits { max_concurrency: 8, max_rps: 20 },     // fair cap for standard/founding
    }
}

#[utoipa::path(post, path = "/internal/agent/authorize", tag = "internal",
    responses((status = 200, description = "Authorized or not")))]
pub async fn authorize(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<AuthorizeRequest>) -> (StatusCode, Json<AuthorizeResponse>) {
    let deny = |_why: &str| (StatusCode::OK, Json(AuthorizeResponse { ok: false, account_id: String::new(), plan: String::new(), limits: limits_for("") }));
    if !service_secret_ok(&state, &headers) {
        return (StatusCode::UNAUTHORIZED, Json(AuthorizeResponse { ok: false, account_id: String::new(), plan: String::new(), limits: limits_for("") }));
    }
    let Some(agent) = agent_for_token(&state, &req.agent_token).await else { return deny("agent"); };
    // Agent must own the requested handle.
    let Some(h) = Handle::find().filter(handle::Column::Name.eq(req.handle.clone())).one(&state.db).await.ok().flatten() else { return deny("handle"); };
    if h.account_id != agent.account_id || h.status != "active" || agent.handle_id != h.id { return deny("ownership"); }
    // Subscription must be active.
    let Some(sub) = active_subscription(&state.db, agent.account_id).await.ok().flatten() else { return deny("sub"); };
    (StatusCode::OK, Json(AuthorizeResponse { ok: true, account_id: agent.account_id.to_string(), plan: sub.plan.clone(), limits: limits_for(&sub.plan) }))
}

#[utoipa::path(post, path = "/internal/key/validate", tag = "internal",
    responses((status = 200, description = "Key validity + sub status")))]
pub async fn key_validate(State(state): State<AppState>, Json(req): Json<KeyValidateRequest>) -> Json<KeyValidateResponse> {
    let invalid = Json(KeyValidateResponse { valid: false, sub_active: false, plan: String::new() });
    // The agent token authenticates the calling machine.
    let Some(agent) = agent_for_token(&state, &req.agent_token).await else { return invalid; };
    // The ak_live_ key must exist, be unrevoked, and belong to the agent's account.
    let hash = token::hash(&req.key);
    let Some(k) = EndpointKey::find().filter(endpoint_key::Column::KeyHash.eq(hash)).one(&state.db).await.ok().flatten() else { return invalid; };
    if k.revoked_at.is_some() || k.account_id != agent.account_id || !token::verify_hash(&req.key, &k.key_hash) {
        return invalid;
    }
    // Best-effort last_used_at touch.
    let mut am: endpoint_key::ActiveModel = k.clone().into();
    am.last_used_at = Set(Some(Utc::now().into()));
    let _ = am.update(&state.db).await;

    let sub = active_subscription(&state.db, agent.account_id).await.ok().flatten();
    match sub {
        Some(s) => Json(KeyValidateResponse { valid: true, sub_active: true, plan: s.plan }),
        None => Json(KeyValidateResponse { valid: true, sub_active: false, plan: String::new() }),
    }
}

#[utoipa::path(post, path = "/internal/agent/heartbeat", tag = "internal",
    responses((status = 200, description = "ok")))]
pub async fn heartbeat(State(state): State<AppState>, Json(req): Json<KeyValidateRequest>) -> StatusCode {
    // Reuse KeyValidateRequest only for its agent_token field is wrong typing; define a tiny struct instead.
    let _ = req;
    StatusCode::OK
}
```
FIX before finishing: `heartbeat` should take a small `{ agent_token }` body, not `KeyValidateRequest`. Define `#[derive(Deserialize)] struct Heartbeat { agent_token: String }` (or add a DTO) and update `agent.last_seen_at`. Implement it properly: resolve the agent, set `last_seen_at = now`, return 200; unknown token → 200 anyway (best-effort, don't leak).

- [ ] **Step 3: Register + lib + app**

`pub mod internal;` in lib.rs. Register the three routes in app.rs via `.routes(routes!(...))`.

- [ ] **Step 4: Truth-table test**

Create `control-plane/tests/internal_validate.rs`: seed account + active subscription + handle + agent + key. Then assert:
- `authorize` WITHOUT the service secret → 401.
- `authorize` with secret + valid agent + owned handle + active sub → `ok=true`, plan set.
- `authorize` with a wrong handle (not owned) → `ok=false`.
- `authorize` with an unknown agent token → `ok=false`.
- After canceling the sub (upsert canceled), `authorize` → `ok=false` (sub gate).
- `key_validate` with valid key + agent + active sub → `valid=true, sub_active=true`.
- `key_validate` with a revoked key → `valid=false`.
- `key_validate` with key from a DIFFERENT account than the agent → `valid=false`.
Set `internal_service_secret: Some("svc-secret".into())` in the test Config and send header `x-altkey-service-secret: svc-secret`.

- [ ] **Step 5: Run + commit**

Run: `cargo test -p control-plane --test internal_validate` → PASS.
```bash
git add control-plane/src/internal control-plane/src/lib.rs control-plane/src/app.rs control-plane/tests/internal_validate.rs
git commit -m "feat(control-plane): internal authorize/key-validate/heartbeat endpoints"
```

---

## Task 5: Rewire the relay's `validate()` → `authorize`

**Files:**
- Modify: `relay/Cargo.toml`, `relay/src/agent_conn.rs`
- Test: `relay/tests/relay_authorize.rs`

- [ ] **Step 1: Deps + config**

In `relay/Cargo.toml` `[dependencies]` add:
```toml
altkey-api = { path = "../altkey-api" }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
```

- [ ] **Step 2: Async validate against the control plane**

In `relay/src/agent_conn.rs`, replace the stub `fn validate(_handle, _token) -> bool { true }` with an async function that calls the control plane WHEN configured, and otherwise (no `CONTROL_PLANE_URL`) accepts — so existing relay tests (`agent_register`, `tunnel_e2e`, `pending_reclaim`) stay green without a control plane:
```rust
use altkey_api::dto::{AuthorizeRequest, AuthorizeResponse};

/// Validate a handle+agent_token at tunnel connect. If CONTROL_PLANE_URL is unset
/// (dev/test), accept (the control plane is the prod authority; tests run without it).
async fn validate(handle: &str, token: &str) -> bool {
    let Ok(base) = std::env::var("CONTROL_PLANE_URL") else { return true; };
    let secret = std::env::var("INTERNAL_SERVICE_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/internal/agent/authorize"))
        .header("x-altkey-service-secret", secret)
        .json(&AuthorizeRequest { handle: handle.to_string(), agent_token: token.to_string() })
        .send().await;
    match resp {
        Ok(r) => r.json::<AuthorizeResponse>().await.map(|a| a.ok).unwrap_or(false),
        Err(e) => { tracing::warn!("authorize call failed: {e}"); false } // fail closed when configured-but-unreachable
    }
}
```
Update the call site in `handle()` to `if !validate(&handle, &token).await { ... reject ... }`. The Hello arm is already async. Reconcile the borrow of `handle`/`token` (clone if needed before the move into the registration).

- [ ] **Step 3: Integration test — relay authorizes against a real control plane**

Create `relay/tests/relay_authorize.rs` (gated on `test-helpers` like `agent_register`): boot a real control-plane app (in-memory SQLite) on an ephemeral port with `internal_service_secret = "svc"`, seed account+sub+handle+agent (use the control-plane crate's store fns — add `control-plane` as a relay dev-dep), set `CONTROL_PLANE_URL` + `INTERNAL_SERVICE_SECRET` env, then drive `agent_conn::handle_for_test` with a Hello carrying the real agent token + handle and assert it registers; and a Hello with a bogus token is rejected. (This replaces the Plan 2 always-true stub with a real authority check.)
RECONCILIATION: env vars are process-global; set them at test start. Add `control-plane = { path = "../control-plane" }` to `relay/[dev-dependencies]`. If wiring a full control-plane boot in the relay test is heavy, an acceptable alternative is a tiny `axum` stub server in the test that returns a canned `AuthorizeResponse` for the expected token and `ok:false` otherwise — the property under test is "relay calls authorize and honors the verdict." Either approach; assert accept-on-ok + reject-on-not-ok.

- [ ] **Step 4: Run + commit**

Run: `cargo test -p altkey-relay --features test-helpers` (all relay tests incl. the new one) → PASS. Also `cargo test -p altkey-relay` (bare) → existing tests still green (validate accepts when CONTROL_PLANE_URL unset).
```bash
git add relay/Cargo.toml relay/src/agent_conn.rs relay/tests/relay_authorize.rs
git commit -m "feat(relay): validate() calls control-plane authorize (fail-closed when configured)"
```

---

## Task 6: Engine `KeyValidator` (control-plane key check + cache + offline grace)

**Files:**
- Create: `engine/src/license.rs`
- Modify: `engine/Cargo.toml`, `engine/src/auth.rs`, `engine/src/lib.rs`, `engine/src/main.rs`
- Test: `engine/tests/license_validator.rs` (or in-file)

- [ ] **Step 1: Dep**

In `engine/Cargo.toml` add `altkey-api = { path = "../altkey-api" }`.

- [ ] **Step 2: KeyValidator with cache + offline grace**

Create `engine/src/license.rs`:
```rust
//! Validate ak_live_ keys against the control plane, with a short positive cache
//! and a bounded offline-grace window so a control-plane blip doesn't instantly
//! kill the local proxy. When the control plane isn't configured, validation falls
//! back to the local key store (dev / transparent mode) via the caller.
use altkey_api::dto::{KeyValidateRequest, KeyValidateResponse};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct ControlPlaneValidator {
    base_url: String,
    agent_token: String,
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, bool)>>,
    ttl: Duration,
    grace: Duration,
    last_ok: Mutex<Option<Instant>>,
}

impl ControlPlaneValidator {
    pub fn new(base_url: String, agent_token: String) -> Self {
        Self {
            base_url, agent_token, http: reqwest::Client::new(),
            cache: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(60),
            grace: Duration::from_secs(72 * 3600),
            last_ok: Mutex::new(None),
        }
    }

    /// True if the key is valid AND the subscription is active. Cached for `ttl`.
    /// On a network error, serve the last cached verdict within `grace`, else fail closed.
    pub async fn validate(&self, key: &str) -> bool {
        if let Some((at, ok)) = self.cache.lock().unwrap().get(key).copied() {
            if at.elapsed() < self.ttl { return ok; }
        }
        let resp = self.http
            .post(format!("{}/internal/key/validate", self.base_url))
            .json(&KeyValidateRequest { key: key.to_string(), agent_token: self.agent_token.clone() })
            .send().await;
        match resp {
            Ok(r) => match r.json::<KeyValidateResponse>().await {
                Ok(v) => {
                    let ok = v.valid && v.sub_active;
                    self.cache.lock().unwrap().insert(key.to_string(), (Instant::now(), ok));
                    *self.last_ok.lock().unwrap() = Some(Instant::now());
                    ok
                }
                Err(_) => false,
            },
            Err(_) => {
                // Offline grace: if we succeeded recently, serve the last cached verdict.
                let within_grace = self.last_ok.lock().unwrap().map(|t| t.elapsed() < self.grace).unwrap_or(false);
                if within_grace {
                    self.cache.lock().unwrap().get(key).map(|(_, ok)| *ok).unwrap_or(false)
                } else {
                    false // fail closed past the grace window
                }
            }
        }
    }
}
```
Add `pub mod license;` to `engine/src/lib.rs` and `mod license;` to `main.rs`.

- [ ] **Step 3: Wire into `require_key` when configured**

Modify `engine/src/auth.rs`: keep the existing local-store path as the default. When `CONTROL_PLANE_URL` + `ALTKEY_AGENT_TOKEN` are set, ALSO require the control-plane validator to approve the key. Read `engine/src/auth.rs` + how `require_key` is called (it's sync today). Two acceptable approaches — pick the one that fits the existing wiring:
  (a) Make the control-plane check available as an async function `pub async fn require_key_cp(state, headers) -> Result<String, (StatusCode, &str)>` used by the request path, holding a shared `ControlPlaneValidator` in the engine's app state; OR
  (b) Add an async pre-check in the request middleware that, when configured, calls a process-global `OnceCell<ControlPlaneValidator>` and rejects with 401/402 on `false`, before the existing sync `require_key`.
The REQUIRED OUTCOME: when `CONTROL_PLANE_URL`+`ALTKEY_AGENT_TOKEN` are set, an `ak_live_` request is served only if the control plane says valid+sub_active (within cache/grace); when UNSET, behavior is exactly as today (local store), so all existing engine tests stay green. Keep it minimal and follow the engine's existing auth wiring. Document the env vars in `engine/src/config.rs` (add `control_plane_url()` + `agent_token()` like the existing `relay_addr()`/`handle()`).

- [ ] **Step 4: Test the validator (cache + grace) with a stub server**

Create `engine/tests/license_validator.rs`: stand up a tiny axum stub returning `KeyValidateResponse { valid:true, sub_active:true }` for a known key and `valid:false` otherwise; assert `validate` returns true/false accordingly and that a second call within TTL doesn't re-hit (optional: count requests). Keep it focused; the offline-grace path can be asserted by pointing at a dead port after a successful call (within grace → still serves last verdict). If grace timing is awkward to test precisely, at minimum assert: valid key → true, invalid key → false, unreachable-from-cold (no prior success) → false (fail closed).

- [ ] **Step 5: Run + commit**

Run: `cargo test -p altkey --test license_validator` → PASS. Then `cargo test -p altkey` (all engine tests still green — the default path is unchanged).
```bash
git add engine/Cargo.toml engine/src/license.rs engine/src/auth.rs engine/src/lib.rs engine/src/main.rs engine/src/config.rs engine/tests/license_validator.rs
git commit -m "feat(engine): control-plane KeyValidator with cache + offline grace"
```

---

## Task 7: Full workspace integration + green

**Files:**
- Test only: `control-plane/tests/*`, run everything

- [ ] **Step 1: Whole-workspace test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test --workspace`
Expected: all crates green — engine (incl. license_validator), relay (bare), control-plane (all registry + internal tests), tunnel-proto, altkey-api.
Run: `cargo test -p altkey-relay --features test-helpers` — relay's feature-gated tests incl. `relay_authorize` PASS.

- [ ] **Step 2: Clippy sweep**

Run: `cargo clippy -p control-plane -p altkey-api --all-targets 2>&1` — fix any NEW warnings in 3d code.

- [ ] **Step 3: Commit any test/clippy fixups**

```bash
git add -A
git commit -m "test(control-plane): 3d full workspace green + clippy"
```
(If nothing to fix, skip.)

---

## Self-Review

**Spec coverage (3d slice):** Implements the spec's registry + the validation authority + the two rewirings: `handle`/`agent`/`endpoint_key` tables + mint-once registry routes (Tasks 2–3); `/internal/agent/authorize` + `/internal/key/validate` + heartbeat with service-secret + agent-token guards and constant-time hash compare (Tasks 1, 4); the relay's `validate()` now calls `authorize` (Task 5, replacing the Plan 2 stub); the engine validates `ak_live_` against `key/validate` with a 60s cache + 72h offline grace, failing closed past it (Task 6). After 3d, a tunnel + a request are gated by a real account + active subscription. Out of scope: usage metering/throughput enforcement (3e), the dashboard UI (3f).

**Placeholder scan:** The `heartbeat` handler in Task 4 Step 2 is shown with a deliberate FIX note (it must take a `{ agent_token }` body and update `last_seen_at`, not reuse `KeyValidateRequest`) — that's an explicit instruction, not a silent gap; the implementer completes it. The engine `require_key` wiring (Task 6 Step 3) offers two concrete approaches with a required OUTCOME and a "keep existing behavior when unconfigured" invariant — real adaptation to the engine's current auth code, which the implementer reads. No "TBD"/vague steps.

**Type consistency:** `AuthorizeRequest/Response`, `KeyValidateRequest/Response`, `Limits` (altkey-api/dto.rs) are the single shared contract used by control-plane internal routes, the relay, and the engine validator. `token::{hash, verify_hash, generate, prefix, TokenKind}` is the one token module. Registry store fns (`claim_handle`, `pair_agent`, `mint_key`) return mint-once plaintext + a stored hash, consistent with how the internal endpoints look them up (by `token::hash`, re-checked with `verify_hash`). The relay's `validate` is async and its call site is awaited. Config accessors mirror existing patterns (`relay_addr()`/`handle()` → add `control_plane_url()`/`agent_token()`).

**Security:** internal endpoints require the service secret (relay) and/or a valid agent token (constant-time); `ak_live_`/`ak_agent_` compared via constant-time `verify_hash` (closes the 3a/3b carry-forward for token validation); cross-account key use blocked (key's account must equal the agent's account); relay + engine **fail closed** when the control plane is configured-but-unreachable beyond grace; default-accept ONLY when the control plane is entirely unconfigured (dev/test). The 3b verified-email gate + 3c webhook-freshness remain separate tracked follow-ups (pre-real-users), unaffected here.
