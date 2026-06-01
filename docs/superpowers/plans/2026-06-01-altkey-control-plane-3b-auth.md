# altkey Control Plane 3b — Auth & Accounts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users sign in to altkey-cloud — via email magic-link or OAuth (Google, Microsoft, GitHub, Apple) — and hold an authenticated session, with `/me` and logout. Accounts key on email; multiple OAuth identities link to one account.

**Architecture:** Builds on 3a's `control-plane` axum crate. First, swap the manual utoipa path registration for `utoipa-axum`'s `OpenApiRouter` so every routed endpoint self-registers in the OpenAPI spec (closes the 3a review's "routed but missing from the contract" seam — load-bearing for the React codegen). Then add auth tables (`identity`, `session`, `magic_link`, `oauth_flow`), a cookie-backed session layer with a `CurrentAccount` extractor, a pluggable `EmailSender` for magic links, a generic OAuth2 (auth-code + PKCE) core with a provider registry, the three standard providers, then Apple (its signed-JWT client secret is a separate task), and finally `/me` + `/auth/logout`.

**Tech Stack:** Rust, axum 0.7, SeaORM 1.1.20, utoipa 5.5 + **utoipa-axum** + utoipa-swagger-ui 8.1, **oauth2 5.x**, **jsonwebtoken 9** (Apple client-secret + id_token), **reqwest** (provider userinfo), cookie via `axum-extra` `CookieJar`, `sha2`/`hex` (token hashing, reuse `altkey-api`), `time`/`chrono`.

**Branch:** Create and work on `feat/control-plane-3b` off `dev`.

---

## File Structure

| File | Responsibility |
|---|---|
| `control-plane/Cargo.toml` | add `utoipa-axum`, `oauth2`, `jsonwebtoken`, `axum-extra` (cookie), `reqwest`, `time` |
| `control-plane/src/app.rs` | rebuild router with `OpenApiRouter`; routes self-register path docs (modify) |
| `control-plane/src/openapi.rs` | `ApiDoc` keeps only `info`/`tags`/security; paths come from the router merge (modify) |
| `control-plane/migration/src/m20260601_000002_create_auth_tables.rs` | identity, session, magic_link, oauth_flow tables |
| `control-plane/migration/src/lib.rs` | register migration #2 (modify) |
| `control-plane/src/entities/{identity,session,magic_link,oauth_flow}.rs` + `mod.rs`,`prelude.rs` | SeaORM entities (modify mod/prelude) |
| `control-plane/src/auth/mod.rs` | auth module root |
| `control-plane/src/auth/session.rs` | issue/lookup/revoke sessions; cookie name + builder |
| `control-plane/src/auth/extract.rs` | `CurrentAccount` axum extractor (cookie → session → account) |
| `control-plane/src/auth/accounts.rs` | `upsert_account_with_identity(email, provider, provider_uid)` |
| `control-plane/src/auth/email.rs` | `EmailSender` trait + `LoggingEmailSender` (dev) + capturing test sender |
| `control-plane/src/auth/magic_link.rs` | request + consume handlers |
| `control-plane/src/auth/oauth/mod.rs` | provider registry + `Provider` config + shared start/callback |
| `control-plane/src/auth/oauth/providers.rs` | Google/Microsoft/GitHub provider definitions |
| `control-plane/src/auth/oauth/apple.rs` | Apple client-secret JWT + id_token email extraction |
| `control-plane/src/routes/me.rs` | `GET /me`, `POST /auth/logout` |
| `control-plane/src/state.rs` | add `email: Arc<dyn EmailSender>` + `oauth: OAuthRegistry` to `AppState` (modify) |
| `control-plane/tests/auth_session.rs` | session issue + extractor + logout |
| `control-plane/tests/auth_magic_link.rs` | request → capture link → consume → session |
| `control-plane/tests/auth_oauth.rs` | OAuth callback with a fake provider → account+identity+session |

**Shared types defined here, reused later (3c–3f):**
```rust
// auth/email.rs
#[async_trait::async_trait]
pub trait EmailSender: Send + Sync {
    async fn send_magic_link(&self, to: &str, link: &str) -> anyhow::Result<()>;
}
// auth/session.rs
pub const SESSION_COOKIE: &str = "altkey_session";
pub async fn issue(db: &DatabaseConnection, account_id: Uuid) -> anyhow::Result<String>; // returns plaintext token
pub async fn account_for(db: &DatabaseConnection, token: &str) -> anyhow::Result<Option<account::Model>>;
pub async fn revoke(db: &DatabaseConnection, token: &str) -> anyhow::Result<()>;
// auth/accounts.rs
pub async fn upsert_account_with_identity(db: &DatabaseConnection, email: &str, provider: &str, provider_uid: &str) -> anyhow::Result<account::Model>;
```

---

## Task 1: Adopt `utoipa-axum` `OpenApiRouter` (close the contract seam)

**Files:**
- Modify: `control-plane/Cargo.toml`, `control-plane/src/app.rs`, `control-plane/src/openapi.rs`, `control-plane/src/routes/health.rs`

- [ ] **Step 1: Add the dependency**

In `control-plane/Cargo.toml` `[dependencies]` add (reconcile the version to the one compatible with utoipa 5.5 — `utoipa-axum` 0.1.x targets utoipa 5; if a newer 0.x is required for utoipa 5.5, use it and report):
```toml
utoipa-axum = "0.1"
```

- [ ] **Step 2: Slim `ApiDoc` to metadata only**

Replace `control-plane/src/openapi.rs`:
```rust
//! Base OpenAPI document: info, tags, and (later) security schemes. Concrete
//! paths + schemas are contributed by each route module via `OpenApiRouter`, so a
//! routed endpoint cannot be missing from the served spec.
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "altkey control plane", version = "0.1.0"),
    tags(
        (name = "system", description = "Service + health"),
        (name = "auth", description = "Login, sessions, identity")
    )
)]
pub struct ApiDoc;
```

- [ ] **Step 3: Rebuild the router with `OpenApiRouter`**

Replace `control-plane/src/app.rs`:
```rust
//! Builds the axum Router via utoipa-axum's OpenApiRouter so every routed handler
//! contributes its own path to the OpenAPI document — the served spec and the route
//! table cannot drift. The merged OpenApi is exposed at /api-docs/openapi.json and
//! rendered by Swagger UI at /swagger-ui.
use crate::openapi::ApiDoc;
use crate::routes;
use crate::state::AppState;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;

pub fn build(state: AppState) -> axum::Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(routes::health::health))
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api))
        .with_state(state)
}
```
RECONCILIATION: `OpenApiRouter::with_openapi(...).routes(routes!(handler)).split_for_parts()` is the utoipa-axum 0.1 API; `split_for_parts()` returns `(axum::Router<S>, utoipa::openapi::OpenApi)`. If the installed version names these differently, adapt — the REQUIRED OUTCOME is unchanged: `/health` is served AND present in `/api-docs/openapi.json`, and the path comes from the router (not a manual `paths()` list). Keep `TraceLayer` if you like (add `.layer(...)` on the final router).

- [ ] **Step 4: Confirm the health route still carries its `#[utoipa::path]`**

`routes/health.rs` already has `#[utoipa::path(get, path = "/health", responses(...), tag = "system")]` on `health` and `Health: ToSchema`. `routes!(health)` reads that attribute. No change needed unless the macro errors; if so, ensure the `#[utoipa::path]` attribute remains directly above `pub async fn health`.

- [ ] **Step 5: Run the existing boot test (unchanged contract)**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test health`
Expected: PASS — `/health` 200 and present in `/api-docs/openapi.json` (the test from 3a still asserts this).

- [ ] **Step 6: Commit**

```bash
git checkout -b feat/control-plane-3b
git add control-plane/Cargo.toml control-plane/src/app.rs control-plane/src/openapi.rs
git commit -m "refactor(control-plane): OpenApiRouter so routes self-register in the spec"
```

---

## Task 2: Auth tables migration + entities

**Files:**
- Create: `control-plane/migration/src/m20260601_000002_create_auth_tables.rs`
- Modify: `control-plane/migration/src/lib.rs`
- Create: `control-plane/src/entities/{identity,session,magic_link,oauth_flow}.rs`
- Modify: `control-plane/src/entities/mod.rs`, `control-plane/src/entities/prelude.rs`
- Test: `control-plane/tests/auth_entities.rs`

- [ ] **Step 1: Write the migration**

Create `control-plane/migration/src/m20260601_000002_create_auth_tables.rs`:
```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Identity {
    Table,
    Id,
    AccountId,
    Provider,
    ProviderUserId,
    EmailAtProvider,
    CreatedAt,
}
#[derive(DeriveIden)]
enum Session {
    Table,
    Id,
    AccountId,
    TokenHash,
    CreatedAt,
    ExpiresAt,
    LastSeenAt,
}
#[derive(DeriveIden)]
enum MagicLink {
    Table,
    Id,
    Email,
    TokenHash,
    ExpiresAt,
    ConsumedAt,
}
#[derive(DeriveIden)]
enum OauthFlow {
    Table,
    State,
    Provider,
    PkceVerifier,
    ReturnTo,
    ExpiresAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create().table(Identity::Table).if_not_exists()
                    .col(ColumnDef::new(Identity::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Identity::AccountId).uuid().not_null())
                    .col(ColumnDef::new(Identity::Provider).string().not_null())
                    .col(ColumnDef::new(Identity::ProviderUserId).string().not_null())
                    .col(ColumnDef::new(Identity::EmailAtProvider).string().null())
                    .col(ColumnDef::new(Identity::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                    .to_owned(),
            ).await?;
        manager.create_index(
            Index::create().name("idx_identity_provider_uid_unique")
                .table(Identity::Table).col(Identity::Provider).col(Identity::ProviderUserId)
                .unique().to_owned(),
        ).await?;

        manager
            .create_table(
                Table::create().table(Session::Table).if_not_exists()
                    .col(ColumnDef::new(Session::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Session::AccountId).uuid().not_null())
                    .col(ColumnDef::new(Session::TokenHash).string().not_null())
                    .col(ColumnDef::new(Session::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                    .col(ColumnDef::new(Session::ExpiresAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Session::LastSeenAt).timestamp_with_time_zone().null())
                    .to_owned(),
            ).await?;
        manager.create_index(
            Index::create().name("idx_session_token_hash_unique")
                .table(Session::Table).col(Session::TokenHash).unique().to_owned(),
        ).await?;

        manager
            .create_table(
                Table::create().table(MagicLink::Table).if_not_exists()
                    .col(ColumnDef::new(MagicLink::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(MagicLink::Email).string().not_null())
                    .col(ColumnDef::new(MagicLink::TokenHash).string().not_null())
                    .col(ColumnDef::new(MagicLink::ExpiresAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(MagicLink::ConsumedAt).timestamp_with_time_zone().null())
                    .to_owned(),
            ).await?;
        manager.create_index(
            Index::create().name("idx_magic_link_token_hash_unique")
                .table(MagicLink::Table).col(MagicLink::TokenHash).unique().to_owned(),
        ).await?;

        manager
            .create_table(
                Table::create().table(OauthFlow::Table).if_not_exists()
                    .col(ColumnDef::new(OauthFlow::State).string().not_null().primary_key())
                    .col(ColumnDef::new(OauthFlow::Provider).string().not_null())
                    .col(ColumnDef::new(OauthFlow::PkceVerifier).string().not_null())
                    .col(ColumnDef::new(OauthFlow::ReturnTo).string().null())
                    .col(ColumnDef::new(OauthFlow::ExpiresAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for t in [OauthFlow::Table.into_iden(), MagicLink::Table.into_iden(), Session::Table.into_iden(), Identity::Table.into_iden()] {
            manager.drop_table(Table::drop().table(t).to_owned()).await?;
        }
        Ok(())
    }
}
```
(If `into_iden()` on the enum variant isn't the right call in 1.1.20, drop each table with an explicit `Table::drop().table(OauthFlow::Table)` etc. — the outcome is all four tables dropped.)

- [ ] **Step 2: Register migration #2**

In `control-plane/migration/src/lib.rs`, add the module + list entry:
```rust
mod m20260601_000001_create_account;
mod m20260601_000002_create_auth_tables;
// ...
fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
        Box::new(m20260601_000001_create_account::Migration),
        Box::new(m20260601_000002_create_auth_tables::Migration),
    ]
}
```

- [ ] **Step 3: Write the entities**

`control-plane/src/entities/identity.rs`:
```rust
//! A linked OAuth identity for an account (provider + provider_user_id).
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub account_id: Uuid,
    pub provider: String,
    pub provider_user_id: String,
    pub email_at_provider: Option<String>,
    pub created_at: DateTimeWithTimeZone,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
```

`control-plane/src/entities/session.rs`:
```rust
//! A browser session: opaque token (hashed) → account, with expiry.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "session")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub account_id: Uuid,
    #[sea_orm(unique)]
    pub token_hash: String,
    pub created_at: DateTimeWithTimeZone,
    pub expires_at: DateTimeWithTimeZone,
    pub last_seen_at: Option<DateTimeWithTimeZone>,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
```

`control-plane/src/entities/magic_link.rs`:
```rust
//! A pending email magic-link: hashed single-use token with a short expiry.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "magic_link")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub email: String,
    #[sea_orm(unique)]
    pub token_hash: String,
    pub expires_at: DateTimeWithTimeZone,
    pub consumed_at: Option<DateTimeWithTimeZone>,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
```

`control-plane/src/entities/oauth_flow.rs`:
```rust
//! Transient CSRF/PKCE state for an in-flight OAuth authorization-code flow.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "oauth_flow")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub state: String,
    pub provider: String,
    pub pkce_verifier: String,
    pub return_to: Option<String>,
    pub expires_at: DateTimeWithTimeZone,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
```

Update `control-plane/src/entities/mod.rs`:
```rust
pub mod account;
pub mod identity;
pub mod magic_link;
pub mod oauth_flow;
pub mod prelude;
pub mod session;
```
Update `control-plane/src/entities/prelude.rs`:
```rust
pub use super::account::Entity as Account;
pub use super::identity::Entity as Identity;
pub use super::magic_link::Entity as MagicLink;
pub use super::oauth_flow::Entity as OauthFlow;
pub use super::session::Entity as Session;
```

- [ ] **Step 4: Test the migration + entities round-trip**

Create `control-plane/tests/auth_entities.rs`:
```rust
//! Migrate in-memory SQLite (both migrations) and round-trip one row per auth table.
use control_plane::entities::{identity, magic_link, oauth_flow, session};
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};

#[tokio::test]
async fn auth_tables_migrate_and_round_trip() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let now = chrono::Utc::now();
    let acct = uuid::Uuid::new_v4();

    identity::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(acct),
        provider: Set("github".into()),
        provider_user_id: Set("123".into()),
        email_at_provider: Set(Some("sen@example.com".into())),
        created_at: Set(now.into()),
    }.insert(&db).await.unwrap();

    session::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(acct),
        token_hash: Set("hash1".into()),
        created_at: Set(now.into()),
        expires_at: Set((now + chrono::Duration::days(30)).into()),
        last_seen_at: Set(None),
    }.insert(&db).await.unwrap();

    magic_link::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        email: Set("sen@example.com".into()),
        token_hash: Set("hash2".into()),
        expires_at: Set((now + chrono::Duration::minutes(15)).into()),
        consumed_at: Set(None),
    }.insert(&db).await.unwrap();

    oauth_flow::ActiveModel {
        state: Set("state-abc".into()),
        provider: Set("google".into()),
        pkce_verifier: Set("verifier".into()),
        return_to: Set(None),
        expires_at: Set((now + chrono::Duration::minutes(10)).into()),
    }.insert(&db).await.unwrap();
}
```

- [ ] **Step 5: Run**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test auth_entities`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add control-plane/migration control-plane/src/entities control-plane/tests/auth_entities.rs
git commit -m "feat(control-plane): auth tables (identity/session/magic_link/oauth_flow) + entities"
```

---

## Task 3: Session layer + `CurrentAccount` extractor

**Files:**
- Modify: `control-plane/Cargo.toml` (add `axum-extra` cookie, `time`)
- Create: `control-plane/src/auth/mod.rs`, `control-plane/src/auth/session.rs`, `control-plane/src/auth/extract.rs`
- Modify: `control-plane/src/lib.rs` (add `pub mod auth;`)
- Test: `control-plane/tests/auth_session.rs`

- [ ] **Step 1: Add deps**

In `control-plane/Cargo.toml`:
```toml
axum-extra = { version = "0.9", features = ["cookie"] }
time = "0.3"
async-trait = "0.1"
```

- [ ] **Step 2: Session store**

Create `control-plane/src/auth/mod.rs`:
```rust
pub mod accounts;
pub mod email;
pub mod extract;
pub mod magic_link;
pub mod oauth;
pub mod session;
```
(Modules `accounts`, `email`, `magic_link`, `oauth` are added in later tasks — to keep this task compiling, create them as empty stubs now: `// filled in a later task` in each. Tasks 4–7 replace them.)

Create `control-plane/src/auth/session.rs`:
```rust
//! Opaque session tokens stored hashed. The cookie carries the plaintext token;
//! the DB stores only its sha256. Lookups join to the account and check expiry.
use crate::entities::{account, prelude::Account, prelude::Session, session};
use altkey_api::token;
use anyhow::Result;
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use rand::Rng;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "altkey_session";
const SESSION_DAYS: i64 = 30;

fn random_token() -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..48).map(|_| A[rng.gen_range(0..A.len())] as char).collect()
}

/// Create a session for `account_id`; returns the plaintext token (store in a cookie).
pub async fn issue(db: &DatabaseConnection, account_id: Uuid) -> Result<String> {
    let plaintext = random_token();
    session::ActiveModel {
        id: Set(Uuid::new_v4()),
        account_id: Set(account_id),
        token_hash: Set(token::hash(&plaintext)),
        created_at: Set(Utc::now().into()),
        expires_at: Set((Utc::now() + Duration::days(SESSION_DAYS)).into()),
        last_seen_at: Set(None),
    }
    .insert(db)
    .await?;
    Ok(plaintext)
}

/// Resolve a plaintext session token to its account, if the session exists and is unexpired.
pub async fn account_for(db: &DatabaseConnection, plaintext: &str) -> Result<Option<account::Model>> {
    let hash = token::hash(plaintext);
    let Some(s) = Session::find()
        .filter(session::Column::TokenHash.eq(hash))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    if s.expires_at < Utc::now() {
        return Ok(None);
    }
    Ok(Account::find_by_id(s.account_id).one(db).await?)
}

/// Revoke (delete) the session for a plaintext token. Idempotent.
pub async fn revoke(db: &DatabaseConnection, plaintext: &str) -> Result<()> {
    let hash = token::hash(plaintext);
    Session::delete_many()
        .filter(session::Column::TokenHash.eq(hash))
        .exec(db)
        .await?;
    Ok(())
}

/// Build the Set-Cookie for a freshly issued session token.
pub fn cookie_for(token: String) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::days(SESSION_DAYS))
        .build()
}

/// Build the Set-Cookie that clears the session (logout).
pub fn clear_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build()
}
```

- [ ] **Step 3: `CurrentAccount` extractor**

Create `control-plane/src/auth/extract.rs`:
```rust
//! Axum extractor that resolves the session cookie to the current account, or 401.
use crate::auth::session::{account_for, SESSION_COOKIE};
use crate::entities::account;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum_extra::extract::CookieJar;

pub struct CurrentAccount(pub account::Model);

impl FromRequestParts<AppState> for CurrentAccount {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get(SESSION_COOKIE).map(|c| c.value().to_string());
        let Some(token) = token else { return Err(StatusCode::UNAUTHORIZED) };
        match account_for(&state.db, &token).await {
            Ok(Some(acct)) => Ok(CurrentAccount(acct)),
            Ok(None) => Err(StatusCode::UNAUTHORIZED),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}
```
RECONCILIATION (axum 0.7): `FromRequestParts` is an async-trait in 0.7 — if the compiler requires `#[async_trait]`, add it; in newer axum it's a native async fn in trait. Match the installed axum 0.7.x. `CookieJar::from_headers` is in `axum-extra` 0.9.

- [ ] **Step 4: Declare `auth` + register in lib.rs**

In `control-plane/src/lib.rs` add `pub mod auth;` (alongside config, db, entities, error, state, routes, app, openapi).

- [ ] **Step 5: Test session issue + lookup + revoke**

Create `control-plane/tests/auth_session.rs`:
```rust
//! Issue a session, resolve it back to the account, then revoke it.
use control_plane::auth::session;
use control_plane::entities::account;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};

#[tokio::test]
async fn session_issue_lookup_revoke() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();

    let id = uuid::Uuid::new_v4();
    account::ActiveModel {
        id: Set(id),
        email: Set("sen@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(chrono::Utc::now().into()),
    }.insert(&db).await.unwrap();

    let token = session::issue(&db, id).await.unwrap();
    let got = session::account_for(&db, &token).await.unwrap();
    assert_eq!(got.unwrap().id, id);

    session::revoke(&db, &token).await.unwrap();
    assert!(session::account_for(&db, &token).await.unwrap().is_none());

    // An unknown token resolves to None, not an error.
    assert!(session::account_for(&db, "bogus").await.unwrap().is_none());
}
```

- [ ] **Step 6: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test auth_session`
Expected: PASS.
```bash
git add control-plane/Cargo.toml control-plane/src/auth control-plane/src/lib.rs control-plane/tests/auth_session.rs
git commit -m "feat(control-plane): cookie-backed sessions + CurrentAccount extractor"
```

---

## Task 4: Account upsert + magic-link request/consume

**Files:**
- Create/replace: `control-plane/src/auth/accounts.rs`, `control-plane/src/auth/email.rs`, `control-plane/src/auth/magic_link.rs`
- Modify: `control-plane/src/state.rs` (add `email` sender)
- Test: `control-plane/tests/auth_magic_link.rs`

- [ ] **Step 1: Account upsert helper**

Replace `control-plane/src/auth/accounts.rs`:
```rust
//! Upsert an account by email and ensure a linked identity row exists. Email is
//! the identity key, so Google-then-GitHub on the same address is ONE account.
use crate::entities::{account, identity, prelude::Account, prelude::Identity};
use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

pub async fn upsert_account_with_identity(
    db: &DatabaseConnection,
    email: &str,
    provider: &str,
    provider_uid: &str,
) -> Result<account::Model> {
    let email = email.trim().to_lowercase();

    let acct = match Account::find()
        .filter(account::Column::Email.eq(email.clone()))
        .one(db)
        .await?
    {
        Some(a) => a,
        None => {
            account::ActiveModel {
                id: Set(Uuid::new_v4()),
                email: Set(email.clone()),
                display_name: Set(None),
                status: Set("active".into()),
                created_at: Set(Utc::now().into()),
            }
            .insert(db)
            .await?
        }
    };

    let existing = Identity::find()
        .filter(identity::Column::Provider.eq(provider))
        .filter(identity::Column::ProviderUserId.eq(provider_uid))
        .one(db)
        .await?;
    if existing.is_none() {
        identity::ActiveModel {
            id: Set(Uuid::new_v4()),
            account_id: Set(acct.id),
            provider: Set(provider.to_string()),
            provider_user_id: Set(provider_uid.to_string()),
            email_at_provider: Set(Some(email.clone())),
            created_at: Set(Utc::now().into()),
        }
        .insert(db)
        .await?;
    }
    Ok(acct)
}
```

- [ ] **Step 2: `EmailSender` trait + impls**

Replace `control-plane/src/auth/email.rs`:
```rust
//! Pluggable email delivery for magic links. Prod wires a real sender (Resend/SMTP);
//! dev logs the link; tests capture it. The trait keeps handlers testable offline.
use anyhow::Result;
use std::sync::{Arc, Mutex};

#[async_trait::async_trait]
pub trait EmailSender: Send + Sync {
    async fn send_magic_link(&self, to: &str, link: &str) -> Result<()>;
}

/// Dev/default: log the magic link instead of sending it.
pub struct LoggingEmailSender;

#[async_trait::async_trait]
impl EmailSender for LoggingEmailSender {
    async fn send_magic_link(&self, to: &str, link: &str) -> Result<()> {
        tracing::info!("magic link for {to}: {link}");
        Ok(())
    }
}

/// Test sender: captures (to, link) pairs for assertions.
#[derive(Clone, Default)]
pub struct CapturingEmailSender {
    pub sent: Arc<Mutex<Vec<(String, String)>>>,
}

#[async_trait::async_trait]
impl EmailSender for CapturingEmailSender {
    async fn send_magic_link(&self, to: &str, link: &str) -> Result<()> {
        self.sent.lock().unwrap().push((to.to_string(), link.to_string()));
        Ok(())
    }
}
```

- [ ] **Step 3: Put the sender in `AppState`**

Modify `control-plane/src/state.rs`:
```rust
//! Shared application state handed to every handler via axum's `State`.
use crate::auth::email::EmailSender;
use crate::config::Config;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Config,
    pub email: Arc<dyn EmailSender>,
}
```
Update `main.rs` boot to supply the dev sender: where `AppState { db, config }` is built, add `email: std::sync::Arc::new(control_plane::auth::email::LoggingEmailSender)`. Update the 3a boot test (`tests/health.rs`) and any other `AppState { .. }` constructions to include `email: std::sync::Arc::new(control_plane::auth::email::LoggingEmailSender)` so they compile.

- [ ] **Step 4: Magic-link handlers**

Replace `control-plane/src/auth/magic_link.rs`:
```rust
//! Email magic-link login: `request` stores a hashed single-use token and emails a
//! link; `consume` verifies it (unexpired, unused), upserts the account, and issues
//! a session cookie.
use crate::auth::{accounts, session};
use crate::entities::{magic_link, prelude::MagicLink};
use crate::error::ApiError;
use crate::state::AppState;
use altkey_api::token;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use rand::Rng;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, utoipa::ToSchema)]
pub struct MagicRequest {
    pub email: String,
}

fn random_token() -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..40).map(|_| A[rng.gen_range(0..A.len())] as char).collect()
}

#[utoipa::path(
    post, path = "/auth/magic-link/request",
    request_body = MagicRequest,
    responses((status = 200, description = "Magic link sent if the email is valid")),
    tag = "auth"
)]
pub async fn request(
    State(state): State<AppState>,
    Json(body): Json<MagicRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let email = body.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::BadRequest("invalid email".into()));
    }
    let plaintext = random_token();
    magic_link::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set(email.clone()),
        token_hash: Set(token::hash(&plaintext)),
        expires_at: Set((Utc::now() + Duration::minutes(15)).into()),
        consumed_at: Set(None),
    }
    .insert(&state.db)
    .await?;

    let link = format!("{}/auth/magic-link/consume?token={}", state.config.public_base_url, plaintext);
    state
        .email
        .send_magic_link(&email, &link)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ConsumeQuery {
    pub token: String,
}

#[utoipa::path(
    get, path = "/auth/magic-link/consume",
    params(("token" = String, Query, description = "The one-time token from the email")),
    responses((status = 303, description = "Session issued; redirect to dashboard")),
    tag = "auth"
)]
pub async fn consume(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(q): Query<ConsumeQuery>,
) -> Result<Response, ApiError> {
    let hash = token::hash(&q.token);
    let row = MagicLink::find()
        .filter(magic_link::Column::TokenHash.eq(hash))
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    if row.consumed_at.is_some() || row.expires_at < Utc::now() {
        return Err(ApiError::Unauthorized);
    }

    // Mark consumed (single-use).
    let mut am: magic_link::ActiveModel = row.clone().into();
    am.consumed_at = Set(Some(Utc::now().into()));
    am.update(&state.db).await?;

    let acct = accounts::upsert_account_with_identity(&state.db, &row.email, "magic_link", &row.email).await?;
    let token = session::issue(&state.db, acct.id).await?;
    let jar = jar.add(session::cookie_for(token));
    Ok((jar, Redirect::to("/")).into_response())
}
```

- [ ] **Step 5: Test request → capture → consume → session**

Create `control-plane/tests/auth_magic_link.rs`:
```rust
//! Drive the magic-link flow end to end against the real handlers + capturing email.
use control_plane::auth::email::CapturingEmailSender;
use control_plane::auth::magic_link::{consume, request, ConsumeQuery, MagicRequest};
use control_plane::config::Config;
use control_plane::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;

fn url_token(link: &str) -> String {
    link.split("token=").nth(1).unwrap().to_string()
}

#[tokio::test]
async fn magic_link_request_then_consume_issues_session() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let email = CapturingEmailSender::default();
    let state = AppState {
        db: db.clone(),
        config: Config {
            database_url: "sqlite::memory:".into(),
            public_base_url: "http://localhost".into(),
            internal_service_secret: None,
            bind_addr: "127.0.0.1:0".into(),
        },
        email: Arc::new(email.clone()),
    };

    request(State(state.clone()), Json(MagicRequest { email: "Sen@Example.com".into() }))
        .await
        .unwrap();

    let (to, link) = email.sent.lock().unwrap()[0].clone();
    assert_eq!(to, "sen@example.com", "email is normalized lowercase");
    let token = url_token(&link);

    let resp = consume(State(state.clone()), CookieJar::new(), Query(ConsumeQuery { token: token.clone() }))
        .await
        .expect("consume succeeds");
    let _ = resp; // a 303 with a Set-Cookie; presence asserted below via a second consume

    // Single-use: consuming the same token again must fail.
    let again = consume(State(state), CookieJar::new(), Query(ConsumeQuery { token })).await;
    assert!(again.is_err(), "second consume must be rejected");
}
```

- [ ] **Step 6: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test auth_magic_link`
Expected: PASS. Also rerun `cargo test -p control-plane --test health` to confirm the `AppState` change didn't break the 3a boot test (you updated its `AppState { .. }` to include `email`).
```bash
git add control-plane/src/auth control-plane/src/state.rs control-plane/src/main.rs control-plane/tests
git commit -m "feat(control-plane): magic-link login + account upsert + EmailSender"
```

---

## Task 5: OAuth core + Google/Microsoft/GitHub

**Files:**
- Replace: `control-plane/src/auth/oauth/mod.rs`
- Create: `control-plane/src/auth/oauth/providers.rs`
- Modify: `control-plane/src/state.rs` (add `oauth: OAuthRegistry`)
- Test: `control-plane/tests/auth_oauth.rs`

- [ ] **Step 1: Provider model + registry + handlers**

Replace `control-plane/src/auth/oauth/mod.rs`:
```rust
//! Generic OAuth2 authorization-code + PKCE login. A `Provider` describes one
//! identity provider; `start` redirects to it (storing CSRF state + PKCE verifier),
//! `callback` exchanges the code, fetches the user's email + id, upserts the
//! account, and issues a session. Apple overrides userinfo (see apple.rs).
pub mod apple;
pub mod providers;

use crate::auth::{accounts, session};
use crate::entities::{oauth_flow, prelude::OauthFlow};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// How to read (provider_user_id, email) out of a provider's userinfo JSON.
#[derive(Clone)]
pub struct Provider {
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scopes: Vec<String>,
    /// JSON pointer-ish field names for id + email in the userinfo response.
    pub id_field: String,
    pub email_field: String,
}

#[derive(Clone, Default)]
pub struct OAuthRegistry {
    pub providers: HashMap<String, Provider>,
}

impl OAuthRegistry {
    pub fn get(&self, name: &str) -> Option<&Provider> {
        self.providers.get(name)
    }
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

/// GET /auth/{provider}/start — build the authorize URL + PKCE, store state, redirect.
pub async fn start(State(state): State<AppState>, Path(provider): Path<String>) -> Result<Response, ApiError> {
    let p = state.oauth.get(&provider).ok_or(ApiError::NotFound)?.clone();
    let (url, csrf, verifier) = providers::authorize_url(&p);
    oauth_flow::ActiveModel {
        state: Set(csrf),
        provider: Set(provider),
        pkce_verifier: Set(verifier),
        return_to: Set(None),
        expires_at: Set((Utc::now() + Duration::minutes(10)).into()),
    }
    .insert(&state.db)
    .await?;
    Ok(Redirect::to(&url).into_response())
}

/// GET /auth/{provider}/callback — validate state, exchange code, fetch userinfo,
/// upsert the account, issue a session cookie.
pub async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    jar: CookieJar,
    Query(q): Query<CallbackQuery>,
) -> Result<Response, ApiError> {
    let p = state.oauth.get(&provider).ok_or(ApiError::NotFound)?.clone();

    let flow = OauthFlow::find()
        .filter(oauth_flow::Column::State.eq(q.state.clone()))
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if flow.provider != provider || flow.expires_at < Utc::now() {
        return Err(ApiError::Unauthorized);
    }
    // One-time: delete the flow row now.
    OauthFlow::delete_by_id(q.state.clone()).exec(&state.db).await?;

    let (uid, email) = if provider == "apple" {
        apple::exchange_and_extract(&p, &q.code, &flow.pkce_verifier)
            .await
            .map_err(ApiError::Internal)?
    } else {
        providers::exchange_and_fetch_userinfo(&p, &q.code, &flow.pkce_verifier)
            .await
            .map_err(ApiError::Internal)?
    };

    let acct = accounts::upsert_account_with_identity(&state.db, &email, &provider, &uid).await?;
    let token = session::issue(&state.db, acct.id).await?;
    let jar = jar.add(session::cookie_for(token));
    Ok((jar, Redirect::to("/")).into_response())
}

/// Build the registry from env (only providers whose client id+secret are set).
pub fn registry_from_env() -> OAuthRegistry {
    let mut providers = HashMap::new();
    for p in providers::from_env() {
        providers.insert(p.name.clone(), p);
    }
    if let Some(a) = apple::from_env() {
        providers.insert(a.name.clone(), a);
    }
    OAuthRegistry { providers }
}

// Re-export Arc for state typing convenience.
pub type SharedRegistry = Arc<OAuthRegistry>;
```

- [ ] **Step 2: Provider definitions + the OAuth2 exchange**

Create `control-plane/src/auth/oauth/providers.rs`:
```rust
//! The three standard providers + the oauth2-crate plumbing for authorize URL and
//! code→token→userinfo. Email/id are read from each provider's userinfo JSON.
use super::Provider;
use anyhow::{anyhow, Result};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
};

fn client(p: &Provider) -> Result<BasicClient> {
    Ok(BasicClient::new(
        ClientId::new(p.client_id.clone()),
        Some(ClientSecret::new(p.client_secret.clone())),
        AuthUrl::new(p.auth_url.clone())?,
        Some(TokenUrl::new(p.token_url.clone())?),
    )
    .set_redirect_uri(RedirectUrl::new(redirect_uri(&p.name))?))
}

fn redirect_uri(provider: &str) -> String {
    // PUBLIC_BASE_URL is read at call time so dev + prod differ without code change.
    let base = std::env::var("PUBLIC_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    format!("{base}/auth/{provider}/callback")
}

/// Returns (authorize_url, csrf_state, pkce_verifier_secret).
pub fn authorize_url(p: &Provider) -> (String, String, String) {
    let c = client(p).expect("valid provider urls");
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut req = c.authorize_url(CsrfToken::new_random).set_pkce_challenge(challenge);
    for s in &p.scopes {
        req = req.add_scope(Scope::new(s.clone()));
    }
    let (url, csrf) = req.url();
    (url.to_string(), csrf.secret().clone(), verifier.secret().clone())
}

/// Exchange the code (with PKCE) and fetch userinfo; return (provider_user_id, email).
pub async fn exchange_and_fetch_userinfo(p: &Provider, code: &str, verifier: &str) -> Result<(String, String)> {
    let c = client(p)?;
    let token = c
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_string()))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|e| anyhow!("token exchange failed: {e}"))?;

    let access = token.access_token().secret().clone();
    let http = reqwest::Client::new();
    let json: serde_json::Value = http
        .get(&p.userinfo_url)
        .bearer_auth(&access)
        .header("User-Agent", "altkey-control-plane")
        .send()
        .await?
        .json()
        .await?;

    let uid = json
        .get(&p.id_field)
        .map(|v| v.to_string().trim_matches('"').to_string())
        .ok_or_else(|| anyhow!("no {} in userinfo", p.id_field))?;
    let email = json
        .get(&p.email_field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("no {} in userinfo", p.email_field))?
        .to_string();
    Ok((uid, email))
}

/// Build Google/Microsoft/GitHub providers from env (only those with id+secret set).
pub fn from_env() -> Vec<Provider> {
    let mut out = Vec::new();
    let mk = |name: &str, auth: &str, token: &str, userinfo: &str, scopes: &[&str], id_field: &str, email_field: &str| -> Option<Provider> {
        let up = name.to_uppercase();
        let client_id = std::env::var(format!("{up}_CLIENT_ID")).ok()?;
        let client_secret = std::env::var(format!("{up}_CLIENT_SECRET")).ok()?;
        Some(Provider {
            name: name.to_string(),
            client_id,
            client_secret,
            auth_url: auth.to_string(),
            token_url: token.to_string(),
            userinfo_url: userinfo.to_string(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            id_field: id_field.to_string(),
            email_field: email_field.to_string(),
        })
    };
    if let Some(p) = mk("google", "https://accounts.google.com/o/oauth2/v2/auth", "https://oauth2.googleapis.com/token", "https://openidconnect.googleapis.com/v1/userinfo", &["openid", "email", "profile"], "sub", "email") { out.push(p); }
    if let Some(p) = mk("microsoft", "https://login.microsoftonline.com/common/oauth2/v2.0/authorize", "https://login.microsoftonline.com/common/oauth2/v2.0/token", "https://graph.microsoft.com/oidc/userinfo", &["openid", "email", "profile"], "sub", "email") { out.push(p); }
    if let Some(p) = mk("github", "https://github.com/login/oauth/authorize", "https://github.com/login/oauth/access_token", "https://api.github.com/user", &["read:user", "user:email"], "id", "email") { out.push(p); }
    out
}
```
RECONCILIATION (oauth2 5.x): the builder may require explicit HTTP-client wiring. In oauth2 5.x, `oauth2::reqwest::async_http_client` may be named differently (e.g. a `reqwest` client passed in) and `BasicClient::new` may take a single arg with builder setters. **Reconcile to the installed oauth2 5.x API; the REQUIRED OUTCOME: `authorize_url` returns a URL+state+verifier, and `exchange_and_fetch_userinfo` turns a code into (uid, email).** Note GitHub's `/user` returns `email: null` when the user's email is private; for v1 that's acceptable (falls to an error → login fails with a clear message). A follow-up can call `/user/emails`. Keep the code shape; only adapt API names.

- [ ] **Step 3: Add the registry to `AppState` + boot**

Modify `control-plane/src/state.rs` to add:
```rust
    pub oauth: std::sync::Arc<crate::auth::oauth::OAuthRegistry>,
```
In `main.rs`, build it: `oauth: std::sync::Arc::new(control_plane::auth::oauth::registry_from_env())`. Update test `AppState { .. }` constructions to add `oauth: std::sync::Arc::new(control_plane::auth::oauth::OAuthRegistry::default())`.

- [ ] **Step 4: Test the callback with a fake provider + a stubbed userinfo**

Because the real token exchange hits the internet, test the part we own: `upsert_account_with_identity` is already covered; here assert the registry + state-validation logic. Create `control-plane/tests/auth_oauth.rs`:
```rust
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
        },
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(OAuthRegistry { providers }),
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
        Query(CallbackQuery { code: "x".into(), state: "never-stored".into() }),
    )
    .await;
    assert!(r.is_err(), "callback with an unknown state must be rejected");
}
```

- [ ] **Step 5: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test auth_oauth`
Expected: PASS (both tests). Also confirm `start` for a known provider builds a redirect (the `unknown_provider_is_404` plus the existing `authorize_url` path compiling covers the happy path without network).
```bash
git add control-plane/src/auth/oauth control-plane/src/state.rs control-plane/src/main.rs control-plane/tests/auth_oauth.rs
git commit -m "feat(control-plane): OAuth2 core + Google/Microsoft/GitHub providers"
```

---

## Task 6: Apple Sign In (signed-JWT client secret + id_token email)

**Files:**
- Replace: `control-plane/src/auth/oauth/apple.rs`
- Modify: `control-plane/Cargo.toml` (add `jsonwebtoken`)
- Test: `control-plane/tests/auth_apple.rs`

- [ ] **Step 1: Add jsonwebtoken**

In `control-plane/Cargo.toml`:
```toml
jsonwebtoken = "9"
```

- [ ] **Step 2: Apple provider + client-secret JWT + id_token extraction**

Replace `control-plane/src/auth/oauth/apple.rs`:
```rust
//! Apple Sign In differs from the standard providers in two ways:
//!  1. the OAuth "client secret" is a short-lived ES256 JWT we sign with a .p8 key
//!     (APPLE_TEAM_ID / APPLE_KEY_ID / APPLE_PRIVATE_KEY / APPLE_CLIENT_ID), and
//!  2. the user's email is a claim inside the returned `id_token` (a JWT), not a
//!     userinfo endpoint.
use super::Provider;
use anyhow::{anyhow, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ClientSecretClaims {
    iss: String, // team id
    iat: i64,
    exp: i64,
    aud: String, // https://appleid.apple.com
    sub: String, // client id (service id)
}

/// Build the ES256 client-secret JWT Apple requires in place of a static secret.
pub fn client_secret_jwt(team_id: &str, key_id: &str, client_id: &str, private_key_pem: &str, now: i64) -> Result<String> {
    let claims = ClientSecretClaims {
        iss: team_id.to_string(),
        iat: now,
        exp: now + 3600,
        aud: "https://appleid.apple.com".into(),
        sub: client_id.to_string(),
    };
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(key_id.to_string());
    let key = EncodingKey::from_ec_pem(private_key_pem.as_bytes())
        .map_err(|e| anyhow!("apple .p8 parse: {e}"))?;
    Ok(encode(&header, &claims, &key)?)
}

#[derive(Deserialize)]
struct IdTokenClaims {
    sub: String,
    email: Option<String>,
}

/// Decode (WITHOUT verifying signature — we just received it over TLS from Apple's
/// token endpoint in direct response to our request) the id_token's sub + email.
/// (A hardened version verifies against Apple's JWKS; acceptable for v1 since the
/// token came straight from the token endpoint, not the browser.)
fn extract_id_token(id_token: &str) -> Result<(String, String)> {
    let mut parts = id_token.split('.');
    let _h = parts.next();
    let payload = parts.next().ok_or_else(|| anyhow!("malformed id_token"))?;
    let bytes = base64_url_decode(payload)?;
    let claims: IdTokenClaims = serde_json::from_slice(&bytes)?;
    let email = claims.email.ok_or_else(|| anyhow!("apple id_token has no email"))?;
    Ok((claims.sub, email))
}

fn base64_url_decode(s: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s)?)
}

/// Exchange the auth code at Apple's token endpoint (using the signed client secret)
/// and return (provider_user_id, email) from the id_token.
pub async fn exchange_and_extract(p: &Provider, code: &str, _verifier: &str) -> Result<(String, String)> {
    let team_id = std::env::var("APPLE_TEAM_ID")?;
    let key_id = std::env::var("APPLE_KEY_ID")?;
    let private_key = std::env::var("APPLE_PRIVATE_KEY")?; // PEM contents of the .p8
    let now = chrono::Utc::now().timestamp();
    let client_secret = client_secret_jwt(&team_id, &key_id, &p.client_id, &private_key, now)?;

    let redirect = {
        let base = std::env::var("PUBLIC_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
        format!("{base}/auth/apple/callback")
    };
    let http = reqwest::Client::new();
    let resp: serde_json::Value = http
        .post(&p.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", &p.client_id),
            ("client_secret", &client_secret),
            ("redirect_uri", &redirect),
        ])
        .send()
        .await?
        .json()
        .await?;
    let id_token = resp
        .get("id_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("apple token response missing id_token: {resp}"))?;
    extract_id_token(id_token)
}

/// Apple provider config from env (APPLE_CLIENT_ID is the Services ID).
pub fn from_env() -> Option<Provider> {
    let client_id = std::env::var("APPLE_CLIENT_ID").ok()?;
    // The .p8 + ids are read at exchange time; presence of CLIENT_ID enables the button.
    Some(Provider {
        name: "apple".into(),
        client_id,
        client_secret: String::new(), // computed per-request (signed JWT)
        auth_url: "https://appleid.apple.com/auth/authorize".into(),
        token_url: "https://appleid.apple.com/auth/token".into(),
        userinfo_url: String::new(), // unused; email comes from id_token
        scopes: vec!["name".into(), "email".into()],
        id_field: "sub".into(),
        email_field: "email".into(),
    })
}
```
Add `base64 = "0.22"` to `control-plane/Cargo.toml`.

RECONCILIATION: `EncodingKey::from_ec_pem` requires jsonwebtoken's default features (ring/pem). If `from_ec_pem` is gated, enable the needed feature. Apple's authorize step also requires `response_mode=form_post` when requesting the `email` scope — when wiring Apple's `start`, the generic `authorize_url` may need `response_mode=form_post` and Apple returns the callback as a POST. For v1, if Apple's form_post callback complicates the GET `callback` handler, note it: Apple may POST to the callback, needing a `post` route variant. Flag this; a minimal approach is to also accept `POST /auth/apple/callback` with form fields `code` + `state`. Implement the POST variant if the GET doesn't suffice — the OUTCOME is Apple login yields (sub, email) → session.

- [ ] **Step 3: Unit-test the client-secret JWT shape (no network)**

Create `control-plane/tests/auth_apple.rs`:
```rust
//! The Apple client-secret JWT must be ES256, carry the key id in the header, and
//! the standard iss/aud/sub claims. We sign with a throwaway EC P-256 key.
use control_plane::auth::oauth::apple::client_secret_jwt;

// A test-only P-256 private key in PKCS#8 PEM (generated for this test; not a secret).
const TEST_P8: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg... (REPLACE: generate with
`openssl ecparam -genkey -name prime256v1 -noout | openssl pkcs8 -topk8 -nocrypt`)
-----END PRIVATE KEY-----";

#[test]
fn client_secret_is_es256_with_kid() {
    let jwt = client_secret_jwt("TEAMID", "KEYID", "com.altkey.service", TEST_P8, 1_700_000_000)
        .expect("sign");
    // Header is base64url JSON with alg ES256 + kid.
    let header_b64 = jwt.split('.').next().unwrap();
    use base64::Engine;
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(header_b64).unwrap();
    let header: serde_json::Value = serde_json::from_slice(&header).unwrap();
    assert_eq!(header["alg"], "ES256");
    assert_eq!(header["kid"], "KEYID");
}
```
IMPLEMENTER NOTE: replace `TEST_P8` with a REAL freshly-generated P-256 PKCS#8 PEM (run the openssl command in the comment, or generate one in-test). The assertion is that the JWT signs and the header is ES256+kid. If embedding a PEM is awkward, generate the key at test time with the `p256`/`pkcs8` crates and serialize to PEM. Do NOT commit a real Apple key — this is a throwaway test key only.

- [ ] **Step 4: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test auth_apple`
Expected: PASS — the JWT is ES256 with the kid header.
```bash
git add control-plane/Cargo.toml control-plane/src/auth/oauth/apple.rs control-plane/tests/auth_apple.rs
git commit -m "feat(control-plane): Apple Sign In — ES256 client-secret JWT + id_token email"
```

---

## Task 7: `/me` + logout + wire all auth routes into the router

**Files:**
- Create: `control-plane/src/routes/me.rs`
- Modify: `control-plane/src/routes/mod.rs`, `control-plane/src/app.rs`
- Test: `control-plane/tests/auth_me.rs`

- [ ] **Step 1: `/me` + `/auth/logout` handlers**

Create `control-plane/src/routes/me.rs`:
```rust
//! `/me` returns the current account (or 401); `/auth/logout` clears the session.
use crate::auth::extract::CurrentAccount;
use crate::auth::session::{self, SESSION_COOKIE};
use crate::state::AppState;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct Me {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
}

#[utoipa::path(
    get, path = "/me",
    responses((status = 200, description = "The current account", body = Me), (status = 401, description = "Not signed in")),
    tag = "auth"
)]
pub async fn me(CurrentAccount(acct): CurrentAccount) -> Json<Me> {
    Json(Me { id: acct.id.to_string(), email: acct.email, display_name: acct.display_name })
}

#[utoipa::path(
    post, path = "/auth/logout",
    responses((status = 200, description = "Session cleared")),
    tag = "auth"
)]
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let _ = session::revoke(&state.db, c.value()).await;
    }
    let jar = jar.add(session::clear_cookie());
    (jar, Json(serde_json::json!({ "ok": true }))).into_response()
}
```

- [ ] **Step 2: Declare the route module**

`control-plane/src/routes/mod.rs`:
```rust
pub mod health;
pub mod me;
```

- [ ] **Step 3: Register all auth + me routes in the router**

Modify `control-plane/src/app.rs` `build` to register the new routes alongside health (using utoipa-axum so each appears in the spec). The function becomes:
```rust
pub fn build(state: AppState) -> axum::Router {
    use crate::auth::{magic_link, oauth};
    use crate::routes;
    use axum::routing::{get, post};

    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(routes::health::health))
        .routes(routes!(routes::me::me))
        .routes(routes!(routes::me::logout))
        .routes(routes!(magic_link::request))
        .routes(routes!(magic_link::consume))
        .split_for_parts();

    // OAuth start/callback are dynamic-path (`/auth/{provider}/...`) and not
    // individually documented as schemas; mount them as plain axum routes.
    let router = router
        .route("/auth/:provider/start", get(oauth::start))
        .route("/auth/:provider/callback", get(oauth::callback));

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api))
        .with_state(state)
}
```
RECONCILIATION: mixing `OpenApiRouter` and plain `.route` on the same `Router<AppState>` is fine — `split_for_parts()` yields a normal `axum::Router<AppState>` you can keep chaining `.route(...)` on. The `:provider` path syntax is axum 0.7's. If Apple needs a POST callback (Task 6 note), also add `.route("/auth/apple/callback", post(oauth::callback))` with a small POST-form adapter, or a dedicated handler — implement per what Apple requires.

- [ ] **Step 4: Integration test — `/me` reflects an issued session, logout clears it**

Create `control-plane/tests/auth_me.rs`:
```rust
//! Boot the real app, manufacture a session for an account, and assert /me returns
//! that account with the cookie and 401 without it; logout then invalidates it.
use control_plane::app;
use control_plane::auth::session;
use control_plane::config::Config;
use control_plane::entities::account;
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};
use std::sync::Arc;

async fn boot() -> (String, sea_orm::DatabaseConnection) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let state = AppState {
        db: db.clone(),
        config: Config {
            database_url: "sqlite::memory:".into(),
            public_base_url: "http://127.0.0.1".into(),
            internal_service_secret: None,
            bind_addr: "127.0.0.1:0".into(),
        },
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
    };
    let appx = app::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, appx).await.unwrap() });
    (format!("http://{addr}"), db)
}

#[tokio::test]
async fn me_requires_session_and_logout_clears_it() {
    let (base, db) = boot().await;

    // Seed an account + a session token.
    let id = uuid::Uuid::new_v4();
    account::ActiveModel {
        id: Set(id), email: Set("sen@example.com".into()), display_name: Set(None),
        status: Set("active".into()), created_at: Set(chrono::Utc::now().into()),
    }.insert(&db).await.unwrap();
    let token = session::issue(&db, id).await.unwrap();

    let client = reqwest::Client::new();

    // No cookie → 401.
    let r = client.get(format!("{base}/me")).send().await.unwrap();
    assert_eq!(r.status(), 401);

    // With cookie → 200 + the account.
    let r = client.get(format!("{base}/me"))
        .header("Cookie", format!("altkey_session={token}"))
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["email"], "sen@example.com");

    // /me is present in the OpenAPI contract (router self-registration).
    let doc: serde_json::Value = client.get(format!("{base}/api-docs/openapi.json"))
        .send().await.unwrap().json().await.unwrap();
    assert!(doc["paths"]["/me"].is_object(), "/me must be in the served spec");

    // Logout revokes the session.
    let r = client.post(format!("{base}/auth/logout"))
        .header("Cookie", format!("altkey_session={token}"))
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    let r = client.get(format!("{base}/me"))
        .header("Cookie", format!("altkey_session={token}"))
        .send().await.unwrap();
    assert_eq!(r.status(), 401, "session must be invalid after logout");
}
```

- [ ] **Step 5: Build + run all control-plane tests + full workspace**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane`
Expected: all control-plane tests PASS (auth_entities, auth_session, auth_magic_link, auth_oauth, auth_apple, auth_me, account_repo, health, config).
Run: `cargo test --workspace`
Expected: all crates green.

- [ ] **Step 6: Commit**

```bash
git add control-plane/src/routes control-plane/src/app.rs control-plane/tests/auth_me.rs
git commit -m "feat(control-plane): /me + logout + auth routes in the OpenAPI router"
```

---

## Self-Review

**Spec coverage (3b slice):** Implements the spec's "Auth + magic-link + sessions" flow: OAuth Authorization-Code + PKCE for Google/Microsoft/GitHub (Task 5) + Apple's signed-JWT client secret + id_token email (Task 6); email magic-link via a pluggable `EmailSender` (Task 4); account upsert by email with linked identities (Task 4); cookie sessions + the `CurrentAccount` extractor (Task 3); `/me` + logout (Task 7). The OpenApiRouter switch (Task 1) closes the 3a-review contract seam so these endpoints can't be missing from the spec. Billing, registry, validation, usage, adapters, React are out of scope (3c–3f).

**Placeholder scan:** No "TBD"/"add validation". Two intentional, flagged reconciliation points carry real instructions, not gaps: (a) oauth2 5.x API names (Task 5) and (b) Apple's `form_post` callback + the throwaway test P-256 key (Task 6). Both specify the required OUTCOME and how to adapt. The Apple test's `TEST_P8` MUST be replaced with a freshly generated throwaway key (never a real Apple key) — called out explicitly.

**Type consistency:** `AppState { db, config, email, oauth }` is constructed identically in main.rs and every test (Tasks 3–7 each update prior `AppState { .. }` sites). `session::{issue, account_for, revoke, cookie_for, clear_cookie, SESSION_COOKIE}` signatures match across session.rs, extract.rs, magic_link.rs, me.rs, and tests. `Provider` fields match between mod.rs, providers.rs, apple.rs, and the oauth test. `upsert_account_with_identity(db, email, provider, provider_uid)` is called identically from magic_link.rs and oauth/mod.rs. `altkey_api::token::hash` is the single hashing function for sessions + magic links.

**Cross-task build ordering:** Task 3 creates empty stubs for `accounts`/`email`/`magic_link`/`oauth` so `auth/mod.rs` compiles; Tasks 4–6 replace them. Each task ends compiling + green. `state.rs` gains `email` in Task 4 and `oauth` in Task 5 — every `AppState` construction site (main.rs + tests) is updated in the SAME task to keep the build green.

**Security notes folded in:** session + magic-link + (later) all tokens stored as sha256 hashes; cookies httpOnly/secure/SameSite=Lax; OAuth state is single-use (deleted on callback) and PKCE-protected; magic links are single-use + 15-min TTL. (Carried from 3a-review for 3d: constant-time compare when validating `ak_live_`/`ak_agent_` hashes — that lands in 3d, not here.)
