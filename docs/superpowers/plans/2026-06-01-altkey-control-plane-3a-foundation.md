# altkey Control Plane 3a — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the `control-plane` axum service and the shared `altkey-api` contract crate — a running, OpenAPI-documented API server backed by SeaORM (SQLite in dev, Postgres in prod) with migrations that run on boot and the first `account` table.

**Architecture:** Two new workspace crates. `altkey-api` is a tiny dependency-light crate holding the token formats (`ak_agent_`, `ak_live_`) and shared DTOs that the engine/relay will later use. `control-plane` is an axum binary: config from env, a SeaORM `DatabaseConnection`, a `migration` module run at startup, route handlers annotated with `utoipa` so `/api-docs/openapi.json` + Swagger UI describe the API. This plan ships the skeleton + health + `account`; later sub-plans (3b–3f) add auth, billing, registry/validation, usage, and the React dashboard.

**Tech Stack:** Rust, axum 0.7, SeaORM 1.x (+ sea-orm-migration), utoipa 5 + utoipa-swagger-ui 8, tokio, serde, anyhow/thiserror, tower-http, rand + sha2 (token hashing), dotenvy (dev env).

**Branch:** Create and work on `feat/control-plane-3a` off `dev`.

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` (root) | add `control-plane`, `altkey-api` to workspace members |
| `altkey-api/Cargo.toml`, `altkey-api/src/lib.rs` | shared contract crate root |
| `altkey-api/src/token.rs` | `ak_agent_` / `ak_live_` token generation, parsing, hashing |
| `control-plane/Cargo.toml` | control-plane crate manifest |
| `control-plane/src/main.rs` | binary entrypoint: load config, connect DB, run migrations, serve |
| `control-plane/src/lib.rs` | library root re-exporting modules (so integration tests can use them) |
| `control-plane/src/config.rs` | env-driven config (`DATABASE_URL`, `PUBLIC_BASE_URL`, `INTERNAL_SERVICE_SECRET`, `BIND_ADDR`) |
| `control-plane/src/db.rs` | build a `DatabaseConnection` from the config |
| `control-plane/src/app.rs` | build the axum `Router` (state + routes + swagger), the single source of the route table |
| `control-plane/src/state.rs` | `AppState { db, config }` shared application state |
| `control-plane/src/routes/health.rs` | `GET /health` handler (utoipa-annotated) |
| `control-plane/src/openapi.rs` | `ApiDoc` (utoipa `OpenApi` derive) aggregating documented paths/schemas |
| `control-plane/src/error.rs` | `ApiError` → `IntoResponse` (uniform JSON errors) |
| `control-plane/migration/Cargo.toml` | sea-orm-migration sub-crate |
| `control-plane/migration/src/lib.rs` | `Migrator` listing migrations |
| `control-plane/migration/src/m20260601_000001_create_account.rs` | first migration: `account` table |
| `control-plane/src/entities/mod.rs`, `control-plane/src/entities/account.rs`, `prelude.rs` | SeaORM entity for `account` |
| `control-plane/tests/health.rs` | integration test: server boots, `/health` 200, openapi.json served |
| `control-plane/tests/account_repo.rs` | integration test: migrate in-memory SQLite, insert+fetch an account |

**Shared types defined here, reused later:**
```rust
// altkey-api/src/token.rs
pub enum TokenKind { Agent, Live }          // ak_agent_ / ak_live_
pub struct Token { pub kind: TokenKind, pub secret: String } // the full plaintext
pub fn generate(kind: TokenKind) -> Token;  // random, prefixed
pub fn prefix(token: &str) -> String;       // first 12 chars, for display
pub fn hash(token: &str) -> String;         // sha256 hex, stored at rest
```

---

## Task 1: Workspace + crate skeletons

**Files:**
- Modify: `Cargo.toml` (root)
- Create: `altkey-api/Cargo.toml`, `altkey-api/src/lib.rs`
- Create: `control-plane/Cargo.toml`, `control-plane/src/main.rs`, `control-plane/src/lib.rs`

- [ ] **Step 1: Add the crates to the workspace**

Edit the root `Cargo.toml` `members` array to include the two new crates. The current file is:
```toml
[workspace]
resolver = "2"
members = ["engine", "relay", "tunnel-proto"]
```
Change it to:
```toml
[workspace]
resolver = "2"
members = ["engine", "relay", "tunnel-proto", "altkey-api", "control-plane", "control-plane/migration"]
```

- [ ] **Step 2: Create `altkey-api`**

`altkey-api/Cargo.toml`:
```toml
[package]
name = "altkey-api"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
rand = "0.8"
sha2 = "0.10"
hex = "0.4"
```

`altkey-api/src/lib.rs`:
```rust
//! Shared contract types for altkey-cloud: token formats and DTOs used by the
//! control plane, the relay, and the engine. Dependency-light on purpose.
pub mod token;
```

- [ ] **Step 3: Create the `control-plane` binary skeleton**

`control-plane/Cargo.toml`:
```toml
[package]
name = "control-plane"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "control-plane"
path = "src/main.rs"

[lib]
name = "control_plane"
path = "src/lib.rs"

[dependencies]
altkey-api = { path = "../altkey-api" }
migration = { path = "migration", package = "control-plane-migration" }
axum = "0.7"
tokio = { version = "1", features = ["full"] }
sea-orm = { version = "1", features = ["sqlx-postgres", "sqlx-sqlite", "runtime-tokio-rustls", "macros", "with-uuid", "with-chrono"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors"] }
utoipa = { version = "5", features = ["axum_extras", "chrono", "uuid"] }
utoipa-swagger-ui = { version = "8", features = ["axum"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
dotenvy = "0.15"

[dev-dependencies]
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
```

`control-plane/src/lib.rs`:
```rust
//! altkey-cloud control plane: accounts, billing, registry, the validation
//! authority, usage, and the OpenAPI/Swagger contract the React app generates from.
pub mod app;
pub mod config;
pub mod db;
pub mod entities;
pub mod error;
pub mod openapi;
pub mod state;
pub mod routes;
```

`control-plane/src/main.rs`:
```rust
//! Binary entrypoint: load env config, connect the DB, run migrations on boot,
//! then serve the axum app.
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("control_plane=info,tower_http=info")),
        )
        .init();

    let config = control_plane::config::Config::from_env()?;
    let db = control_plane::db::connect(&config).await?;
    control_plane::db::run_migrations(&db).await?;

    let app = control_plane::app::build(control_plane::state::AppState { db, config: config.clone() });
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("control-plane listening on {}", config.bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
```

(The modules referenced above are created in the following tasks. To keep `main.rs`
compiling at the end of this task only, you may temporarily comment the body of `main`
down to `Ok(())` — but it's cleaner to land Tasks 2–7 before a full `cargo build` of the
binary. Build just the crate skeletons here.)

- [ ] **Step 4: Verify the new crates are recognized**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo metadata --no-deps --format-version 1 | tr ',' '\n' | grep -E "altkey-api|control-plane"`
Expected: both `altkey-api` and `control-plane` (and `control-plane-migration`, added in Task 5) appear as workspace packages. (The full `cargo build` happens after Task 7 once all modules exist.)

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/control-plane-3a
git add Cargo.toml altkey-api control-plane/Cargo.toml control-plane/src/main.rs control-plane/src/lib.rs
git commit -m "build(control-plane): workspace crates skeleton (altkey-api + control-plane)"
```

---

## Task 2: Token formats in `altkey-api`

**Files:**
- Create: `altkey-api/src/token.rs`

- [ ] **Step 1: Write the failing test + implementation**

Create `altkey-api/src/token.rs`:
```rust
//! Token formats for altkey-cloud. Two kinds, both `<prefix><random>`:
//! - `ak_agent_…` identifies one paired machine to the cloud (relay + validation API).
//! - `ak_live_…`  is the endpoint key a calling app sends; the agent validates it.
//! Secrets are shown to the user exactly once; the cloud stores only `hash()`.
use rand::Rng;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Agent,
    Live,
}

impl TokenKind {
    pub fn prefix_str(self) -> &'static str {
        match self {
            TokenKind::Agent => "ak_agent_",
            TokenKind::Live => "ak_live_",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    /// The full plaintext token including the kind prefix. Show once, never stored.
    pub secret: String,
}

/// 32 bytes of randomness, base32-ish lowercase alnum, with the kind prefix.
pub fn generate(kind: TokenKind) -> Token {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let body: String = (0..40)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    Token { kind, secret: format!("{}{}", kind.prefix_str(), body) }
}

/// A short, safe-to-display prefix of a token (kind prefix + first 6 random chars).
pub fn prefix(token: &str) -> String {
    token.chars().take(15).collect()
}

/// SHA-256 hex digest — what gets stored at rest. Validation hashes the presented
/// token and compares to the stored hash (constant-work comparison via fixed-length hex).
pub fn hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

/// Classify a presented token by its prefix, if recognized.
pub fn kind_of(token: &str) -> Option<TokenKind> {
    if token.starts_with(TokenKind::Agent.prefix_str()) {
        Some(TokenKind::Agent)
    } else if token.starts_with(TokenKind::Live.prefix_str()) {
        Some(TokenKind::Live)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_has_prefix_and_is_unique() {
        let a = generate(TokenKind::Agent);
        let b = generate(TokenKind::Agent);
        assert!(a.secret.starts_with("ak_agent_"));
        assert_eq!(a.kind, TokenKind::Agent);
        assert_ne!(a.secret, b.secret, "tokens must be random");
        assert!(generate(TokenKind::Live).secret.starts_with("ak_live_"));
    }

    #[test]
    fn hash_is_stable_and_prefix_is_short() {
        let t = generate(TokenKind::Live);
        assert_eq!(hash(&t.secret), hash(&t.secret), "hash is deterministic");
        assert_ne!(hash(&t.secret), t.secret, "hash != plaintext");
        assert_eq!(hash(&t.secret).len(), 64, "sha256 hex is 64 chars");
        assert_eq!(prefix(&t.secret), &t.secret[..15]);
    }

    #[test]
    fn kind_of_classifies() {
        assert_eq!(kind_of("ak_agent_xyz"), Some(TokenKind::Agent));
        assert_eq!(kind_of("ak_live_xyz"), Some(TokenKind::Live));
        assert_eq!(kind_of("nope"), None);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p altkey-api`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add altkey-api/src/token.rs
git commit -m "feat(altkey-api): ak_agent_/ak_live_ token generate, prefix, hash"
```

---

## Task 3: Config from env

**Files:**
- Create: `control-plane/src/config.rs`

- [ ] **Step 1: Write the failing test + implementation**

Create `control-plane/src/config.rs`:
```rust
//! Process configuration, all from the environment (12-factor). Dev defaults to a
//! local SQLite file so the server runs with zero setup; prod sets DATABASE_URL to
//! a Postgres URL. No secrets are hardcoded.
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Config {
    /// SeaORM connection URL. `sqlite://…` or `postgres://…`.
    pub database_url: String,
    /// Public base URL of this service (used to build OAuth redirect URIs later).
    pub public_base_url: String,
    /// Shared secret the relay presents on /internal/* calls (set in 3d; optional now).
    pub internal_service_secret: Option<String>,
    /// Address to bind the HTTP server.
    pub bind_addr: String,
}

impl Config {
    pub fn from_env() -> Result<Config> {
        Ok(Config {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://./control-plane.db?mode=rwc".into()),
            public_base_url: std::env::var("PUBLIC_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            internal_service_secret: std::env::var("INTERNAL_SERVICE_SECRET").ok(),
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane_when_env_absent() {
        // Clear the vars this test cares about so defaults apply deterministically.
        for k in ["DATABASE_URL", "PUBLIC_BASE_URL", "BIND_ADDR", "INTERNAL_SERVICE_SECRET"] {
            std::env::remove_var(k);
        }
        let c = Config::from_env().unwrap();
        assert!(c.database_url.starts_with("sqlite://"));
        assert_eq!(c.bind_addr, "127.0.0.1:8080");
        assert!(c.internal_service_secret.is_none());
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane config::`
Expected: PASS (the crate won't fully build until later modules exist; if the crate fails to compile because other modules are missing, land Tasks 4–7 first then run — but `config.rs` itself has no internal deps, so prefer to also stub empty modules per Task 1 Step 3's note). To run this in isolation, ensure `lib.rs` only declares modules that exist; temporarily trim `lib.rs` to `pub mod config;` while iterating, restoring the full list by Task 7.

- [ ] **Step 3: Commit**

```bash
git add control-plane/src/config.rs
git commit -m "feat(control-plane): env-driven Config with dev SQLite default"
```

---

## Task 4: SeaORM migration sub-crate + Migrator

**Files:**
- Create: `control-plane/migration/Cargo.toml`
- Create: `control-plane/migration/src/lib.rs`

- [ ] **Step 1: Create the migration crate manifest**

`control-plane/migration/Cargo.toml`:
```toml
[package]
name = "control-plane-migration"
version = "0.1.0"
edition = "2021"

[lib]
name = "migration"
path = "src/lib.rs"

[dependencies]
sea-orm-migration = { version = "1", features = ["runtime-tokio-rustls", "sqlx-postgres", "sqlx-sqlite"] }
async-trait = "0.1"
```

- [ ] **Step 2: Create the Migrator listing (empty for now)**

`control-plane/migration/src/lib.rs`:
```rust
//! SeaORM migrations for the control plane. Each migration is portable across
//! Postgres (prod) and SQLite (dev): string-valued enums, no PG-only column types.
pub use sea_orm_migration::prelude::*;

mod m20260601_000001_create_account;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260601_000001_create_account::Migration)]
    }
}
```

(The migration module file is created in Task 5. This file references it; both land
together for a clean compile — implement Task 5 immediately after this step before
building.)

- [ ] **Step 3: Commit (with Task 5)**

Defer the commit to the end of Task 5 so the migration crate compiles.

---

## Task 5: First migration + `account` entity

**Files:**
- Create: `control-plane/migration/src/m20260601_000001_create_account.rs`
- Create: `control-plane/src/entities/mod.rs`, `control-plane/src/entities/prelude.rs`, `control-plane/src/entities/account.rs`
- Create: `control-plane/src/db.rs`
- Test: `control-plane/tests/account_repo.rs`

- [ ] **Step 1: Write the `account` migration**

Create `control-plane/migration/src/m20260601_000001_create_account.rs`:
```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Account {
    Table,
    Id,
    Email,
    DisplayName,
    Status,
    CreatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Account::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Account::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Account::Email).string().not_null())
                    .col(ColumnDef::new(Account::DisplayName).string().null())
                    .col(ColumnDef::new(Account::Status).string().not_null().default("active"))
                    .col(
                        ColumnDef::new(Account::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_account_email_unique")
                    .table(Account::Table)
                    .col(Account::Email)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Account::Table).to_owned()).await
    }
}
```

- [ ] **Step 2: Write the `account` entity**

Create `control-plane/src/entities/account.rs`:
```rust
//! SeaORM entity for the `account` table — the identity key for everything else.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "account")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub email: String,
    pub display_name: Option<String>,
    pub status: String,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

Create `control-plane/src/entities/prelude.rs`:
```rust
pub use super::account::Entity as Account;
```

Create `control-plane/src/entities/mod.rs`:
```rust
pub mod account;
pub mod prelude;
```

- [ ] **Step 3: Write the DB connect + migration runner**

Create `control-plane/src/db.rs`:
```rust
//! Database connection + boot-time migration runner. One `connect` builds a
//! SeaORM `DatabaseConnection` from the config URL (SQLite or Postgres).
use crate::config::Config;
use anyhow::Result;
use migration::{Migrator, MigratorTrait};
use sea_orm::{Database, DatabaseConnection};

pub async fn connect(config: &Config) -> Result<DatabaseConnection> {
    let db = Database::connect(&config.database_url).await?;
    Ok(db)
}

pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    Migrator::up(db, None).await?;
    Ok(())
}
```

- [ ] **Step 4: Write the integration test**

Create `control-plane/tests/account_repo.rs`:
```rust
//! Boot an in-memory SQLite, run migrations, insert + fetch an account.
use control_plane::entities::{account, prelude::Account};
use sea_orm::{ActiveModelTrait, Database, EntityTrait, Set};

#[tokio::test]
async fn migrate_then_insert_and_fetch_account() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let id = uuid::Uuid::new_v4();
    account::ActiveModel {
        id: Set(id),
        email: Set("sen@example.com".into()),
        display_name: Set(Some("Sen".into())),
        status: Set("active".into()),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let got = Account::find_by_id(id).one(&db).await.unwrap().unwrap();
    assert_eq!(got.email, "sen@example.com");
    assert_eq!(got.status, "active");
}
```

Add `migration` + `uuid` + `chrono` as needed to `control-plane`'s deps (already present
from Task 1; `migration` is a normal dep so tests can call `Migrator`).

- [ ] **Step 5: Run the test**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test account_repo`
Expected: PASS — migration creates `account`, insert + fetch round-trips.

- [ ] **Step 6: Commit**

```bash
git add control-plane/migration control-plane/src/entities control-plane/src/db.rs control-plane/tests/account_repo.rs
git commit -m "feat(control-plane): account migration + entity + boot migration runner"
```

---

## Task 6: Error type, state, app router, health route

**Files:**
- Create: `control-plane/src/error.rs`
- Create: `control-plane/src/state.rs`
- Create: `control-plane/src/routes/mod.rs`, `control-plane/src/routes/health.rs`
- Create: `control-plane/src/app.rs`

- [ ] **Step 1: Write the uniform error type**

Create `control-plane/src/error.rs`:
```rust
//! One error type for all handlers → uniform JSON `{ "error": { "code", "message" } }`.
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        if let ApiError::Internal(ref e) = self {
            tracing::error!("internal error: {e:#}");
        }
        (status, Json(json!({ "error": { "code": code, "message": self.to_string() } }))).into_response()
    }
}

impl From<sea_orm::DbErr> for ApiError {
    fn from(e: sea_orm::DbErr) -> Self {
        ApiError::Internal(anyhow::anyhow!(e))
    }
}
```

- [ ] **Step 2: Write the shared app state**

Create `control-plane/src/state.rs`:
```rust
//! Shared application state handed to every handler via axum's `State`.
use crate::config::Config;
use sea_orm::DatabaseConnection;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Config,
}
```

- [ ] **Step 3: Write the health route**

Create `control-plane/src/routes/mod.rs`:
```rust
pub mod health;
```

Create `control-plane/src/routes/health.rs`:
```rust
//! Liveness probe + a tiny DB ping so /health reflects DB connectivity.
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use sea_orm::{ConnectionTrait, Statement};
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct Health {
    pub status: String,
    pub db: bool,
}

#[utoipa::path(
    get,
    path = "/health",
    responses((status = 200, description = "Service health", body = Health)),
    tag = "system"
)]
pub async fn health(State(state): State<AppState>) -> Json<Health> {
    let db_ok = state
        .db
        .execute(Statement::from_string(state.db.get_database_backend(), "SELECT 1".to_owned()))
        .await
        .is_ok();
    Json(Health { status: "ok".into(), db: db_ok })
}
```

- [ ] **Step 4: Write the app router (no swagger yet — added in Task 7)**

Create `control-plane/src/app.rs`:
```rust
//! Builds the axum Router: shared state, routes, middleware. The Swagger UI +
//! OpenAPI JSON are merged in Task 7 so this is the single route-table source.
use crate::routes;
use crate::state::AppState;
use axum::routing::get;
use axum::Router;
use tower_http::trace::TraceLayer;

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health::health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

- [ ] **Step 5: Build the crate**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo build -p control-plane`
Expected: compiles (openapi.rs is added next; `lib.rs` currently lists `openapi` — temporarily comment `pub mod openapi;` in `lib.rs` until Task 7, or implement Task 7 before building). Prefer to implement Task 7 next, then build once.

- [ ] **Step 6: Commit**

```bash
git add control-plane/src/error.rs control-plane/src/state.rs control-plane/src/routes control-plane/src/app.rs
git commit -m "feat(control-plane): ApiError, AppState, health route, app router"
```

---

## Task 7: OpenAPI doc + Swagger UI + boot test

**Files:**
- Create: `control-plane/src/openapi.rs`
- Modify: `control-plane/src/app.rs` (merge Swagger UI + openapi.json)
- Test: `control-plane/tests/health.rs`

- [ ] **Step 1: Write the OpenAPI aggregator**

Create `control-plane/src/openapi.rs`:
```rust
//! The single utoipa `OpenApi` document. Every documented handler is listed in
//! `paths(...)`; every response/request schema in `components(schemas(...))`.
//! This is the contract the React app generates its client from — keep it complete.
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "altkey control plane", version = "0.1.0"),
    paths(crate::routes::health::health),
    components(schemas(crate::routes::health::Health)),
    tags(
        (name = "system", description = "Service + health")
    )
)]
pub struct ApiDoc;
```

- [ ] **Step 2: Merge Swagger UI + openapi.json into the router**

Replace `control-plane/src/app.rs` with:
```rust
//! Builds the axum Router: shared state, routes, middleware, and the OpenAPI
//! contract surface (`/api-docs/openapi.json` + `/swagger-ui`).
use crate::openapi::ApiDoc;
use crate::routes;
use crate::state::AppState;
use axum::routing::get;
use axum::Router;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health::health))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

- [ ] **Step 3: Write the boot/integration test**

Create `control-plane/tests/health.rs`:
```rust
//! Boot the real app on an ephemeral port against in-memory SQLite and hit
//! /health and /api-docs/openapi.json over real HTTP.
use control_plane::app;
use control_plane::config::Config;
use control_plane::state::AppState;
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
    let appx = app::build(AppState { db, config });
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

    let oa = client.get(format!("{base}/api-docs/openapi.json")).send().await.unwrap();
    assert_eq!(oa.status(), 200);
    let doc: serde_json::Value = oa.json().await.unwrap();
    assert_eq!(doc["info"]["title"], "altkey control plane");
    assert!(doc["paths"]["/health"].is_object(), "/health must be in the OpenAPI contract");
}
```

- [ ] **Step 4: Build + run all control-plane tests**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo build -p control-plane && cargo test -p control-plane`
Expected: compiles; `account_repo` + `health` tests PASS; `/health` is present in the served openapi.json.

- [ ] **Step 5: Full workspace check (nothing else broke)**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test --workspace`
Expected: all prior crates (engine, relay, tunnel-proto, altkey-api) + control-plane green.

- [ ] **Step 6: Commit**

```bash
git add control-plane/src/openapi.rs control-plane/src/app.rs control-plane/tests/health.rs
git commit -m "feat(control-plane): OpenAPI doc + Swagger UI + boot/health integration test"
```

---

## Self-Review

**Spec coverage (3a's slice):** the spec's "Architecture → Components inside control-plane"
and "Deployment (12-factor)" are realized: `control-plane` crate + `altkey-api` contract
crate (Task 1), env config with SQLite-dev/Postgres-prod (Task 3), SeaORM + migrations on
boot (Tasks 4–5), the `account` table from the data model (Task 5), the OpenAPI/Swagger
contract surface the React app will generate from (Task 7). Auth, billing, registry,
validation, usage, adapters, and the React app are explicitly deferred to 3b–3f and are NOT
in this plan's scope.

**Placeholder scan:** no "TBD"/"add error handling"/"similar to Task N". The only
intentional notes are build-ordering hints (some modules reference siblings created in the
next task) — these are real instructions to land paired tasks together before a full build,
not missing content. Every code step shows complete code.

**Type consistency:** `Config` fields (`database_url`, `public_base_url`,
`internal_service_secret`, `bind_addr`) are identical across config.rs, main.rs, and the
health test. `AppState { db, config }` matches in state.rs, main.rs, app.rs, and the test.
`account::ActiveModel` fields (`id, email, display_name, status, created_at`) match the
migration columns and the entity `Model`. `Migrator` is referenced consistently via the
`migration` crate (package `control-plane-migration`, lib name `migration`).

**Known reconciliation points (versions may have drifted — reconcile to installed):**
1. **SeaORM 1.x** entity macro + `timestamp_with_time_zone()` map to `DateTimeWithTimeZone`;
   if the installed SeaORM differs, align the entity field type and the migration column
   builder. `Expr::current_timestamp()` is the portable default.
2. **utoipa 5 / utoipa-swagger-ui 8** — the `SwaggerUi::new(...).url(...)` + `ApiDoc::openapi()`
   wiring and the `axum` feature flags are version-sensitive; if the builder differs, match
   the installed crate's axum integration (the property under test is "openapi.json served +
   /health documented").
3. **sea-orm-migration** `MigratorTrait`/`MigrationTrait` + `DeriveMigrationName`/`DeriveIden`
   are stable in 1.x; `async-trait` is required on the impls.
4. SQLite URL forms: tests use `sqlite::memory:`; the dev default uses
   `sqlite://./control-plane.db?mode=rwc` (file, auto-created). Keep `control-plane.db`
   gitignored.

Add `control-plane.db` (and `*.db-journal`/`-wal`) to `.gitignore` during Task 1 if not
already covered.
