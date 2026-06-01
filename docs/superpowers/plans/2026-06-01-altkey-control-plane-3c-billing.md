# altkey Control Plane 3c — Billing (Polar) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Subscriptions via Polar — a user starts a checkout for a plan, Polar (merchant-of-record) drives payment, and a signed webhook updates the account's `subscription` row, which becomes the **license gate** every other part of altkey reads to decide "active subscriber or not."

**Architecture:** Adds a `subscription` table + entity, a Polar webhook receiver that verifies a Standard-Webhooks HMAC signature and upserts subscription state, a `PolarClient` trait (real reqwest impl + test fake) for creating checkout + customer-portal sessions, and `/billing/*` endpoints. The license-gate helper `active_subscription(db, account_id)` is the single source of truth 3d (validation) and the dashboard consume. No card data ever touches altkey.

**Tech Stack:** Rust, axum 0.7, SeaORM 1.1.20, reqwest (Polar REST), `hmac` + `sha2` + `base64` (webhook signature), utoipa-axum. No official Polar Rust SDK — we call Polar's REST API directly.

**Branch:** `feat/control-plane-3c` off `dev`.

---

## File Structure

| File | Responsibility |
|---|---|
| `control-plane/Cargo.toml` | add `hmac`, `subtle` (constant-time), `base64` already present? add if missing |
| `control-plane/migration/src/m20260601_000003_create_subscription.rs` | subscription table |
| `control-plane/migration/src/lib.rs` | register migration #3 (modify) |
| `control-plane/src/entities/subscription.rs` + mod/prelude | SeaORM entity (modify mod/prelude) |
| `control-plane/src/billing/mod.rs` | module root |
| `control-plane/src/billing/plan.rs` | `Plan` enum + product-id↔plan mapping from config |
| `control-plane/src/billing/store.rs` | `active_subscription`, `upsert_from_polar` (the license gate) |
| `control-plane/src/billing/webhook_sig.rs` | Standard-Webhooks HMAC verify |
| `control-plane/src/billing/webhook.rs` | `POST /webhooks/polar` handler |
| `control-plane/src/billing/polar.rs` | `PolarClient` trait + `HttpPolarClient` + `FakePolarClient` |
| `control-plane/src/billing/routes.rs` | `/billing/checkout`, `/billing/portal`, `/billing/subscription` |
| `control-plane/src/config.rs` | add Polar config (modify) |
| `control-plane/src/state.rs` | add `polar: Arc<dyn PolarClient>` (modify) |
| `control-plane/src/app.rs` | register billing routes (modify) |
| `control-plane/src/lib.rs` | `pub mod billing;` (modify) |
| `control-plane/tests/billing_*.rs` | webhook sig, webhook→subscription, license gate, checkout |

**Shared types defined here (3d consumes `active_subscription`):**
```rust
// billing/plan.rs
pub enum Plan { Founding, Standard, Pro }   // string values: "founding"/"standard"/"pro"
// billing/store.rs
pub async fn active_subscription(db: &DatabaseConnection, account_id: Uuid) -> anyhow::Result<Option<subscription::Model>>;
pub async fn upsert_from_polar(db: &DatabaseConnection, ev: &PolarSubscriptionEvent) -> anyhow::Result<()>;
// billing/polar.rs
#[async_trait] pub trait PolarClient: Send + Sync {
    async fn create_checkout(&self, account_id: Uuid, plan: Plan, success_url: &str) -> anyhow::Result<String>; // returns checkout URL
    async fn customer_portal_url(&self, polar_customer_id: &str) -> anyhow::Result<String>;
}
```

---

## Task 1: Subscription migration + entity

**Files:**
- Create: `control-plane/migration/src/m20260601_000003_create_subscription.rs`
- Modify: `control-plane/migration/src/lib.rs`
- Create: `control-plane/src/entities/subscription.rs`; modify `entities/mod.rs`, `prelude.rs`
- Test: `control-plane/tests/billing_entity.rs`

- [ ] **Step 1: Migration**

Create `control-plane/migration/src/m20260601_000003_create_subscription.rs`:
```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Subscription {
    Table,
    Id,
    AccountId,
    PolarCustomerId,
    PolarSubscriptionId,
    Plan,
    Status,
    CurrentPeriodEnd,
    IsFounding,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(
            Table::create().table(Subscription::Table).if_not_exists()
                .col(ColumnDef::new(Subscription::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(Subscription::AccountId).uuid().not_null())
                .col(ColumnDef::new(Subscription::PolarCustomerId).string().null())
                .col(ColumnDef::new(Subscription::PolarSubscriptionId).string().null())
                .col(ColumnDef::new(Subscription::Plan).string().not_null())
                .col(ColumnDef::new(Subscription::Status).string().not_null())
                .col(ColumnDef::new(Subscription::CurrentPeriodEnd).timestamp_with_time_zone().null())
                .col(ColumnDef::new(Subscription::IsFounding).boolean().not_null().default(false))
                .col(ColumnDef::new(Subscription::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Subscription::UpdatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                .to_owned(),
        ).await?;
        // One active subscription row per account (we upsert by account).
        manager.create_index(
            Index::create().name("idx_subscription_account_unique")
                .table(Subscription::Table).col(Subscription::AccountId).unique().to_owned(),
        ).await?;
        // Look up by Polar subscription id from webhooks.
        manager.create_index(
            Index::create().name("idx_subscription_polar_sub")
                .table(Subscription::Table).col(Subscription::PolarSubscriptionId).to_owned(),
        ).await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Subscription::Table).to_owned()).await
    }
}
```

- [ ] **Step 2: Register migration #3**

In `control-plane/migration/src/lib.rs` add `mod m20260601_000003_create_subscription;` and append `Box::new(m20260601_000003_create_subscription::Migration)` to the `migrations()` vec (after #2).

- [ ] **Step 3: Entity**

Create `control-plane/src/entities/subscription.rs`:
```rust
//! SeaORM entity for `subscription` — the license gate. One row per account.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "subscription")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub account_id: Uuid,
    pub polar_customer_id: Option<String>,
    pub polar_subscription_id: Option<String>,
    pub plan: String,
    pub status: String,
    pub current_period_end: Option<DateTimeWithTimeZone>,
    pub is_founding: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
```
Add `pub mod subscription;` to `entities/mod.rs` and `pub use super::subscription::Entity as Subscription;` to `prelude.rs`.

- [ ] **Step 4: Test**

Create `control-plane/tests/billing_entity.rs`:
```rust
//! Migrate (all migrations) on SQLite and round-trip a subscription row.
use control_plane::entities::subscription;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};

#[tokio::test]
async fn subscription_migrates_and_round_trips() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let now = chrono::Utc::now();
    subscription::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        account_id: Set(uuid::Uuid::new_v4()),
        polar_customer_id: Set(Some("cus_1".into())),
        polar_subscription_id: Set(Some("sub_1".into())),
        plan: Set("standard".into()),
        status: Set("active".into()),
        current_period_end: Set(Some((now + chrono::Duration::days(30)).into())),
        is_founding: Set(false),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }.insert(&db).await.unwrap();
}
```

- [ ] **Step 5: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test billing_entity`
Expected: PASS.
```bash
git checkout -b feat/control-plane-3c
git add control-plane/migration control-plane/src/entities control-plane/tests/billing_entity.rs
git commit -m "feat(control-plane): subscription table + entity"
```

---

## Task 2: Plan mapping + license-gate store

**Files:**
- Create: `control-plane/src/billing/mod.rs`, `control-plane/src/billing/plan.rs`, `control-plane/src/billing/store.rs`
- Modify: `control-plane/src/lib.rs` (`pub mod billing;`)
- Modify: `control-plane/src/config.rs` (Polar config)
- Test: `control-plane/tests/billing_gate.rs`

- [ ] **Step 1: Polar config**

Append to `control-plane/src/config.rs` `Config` struct + `from_env`:
```rust
    // --- Polar billing (3c) ---
    pub polar_access_token: Option<String>,
    pub polar_webhook_secret: Option<String>,
    pub polar_base_url: String,
    /// Polar product IDs per plan (so a webhook product maps back to our Plan).
    pub polar_product_founding: Option<String>,
    pub polar_product_standard: Option<String>,
    pub polar_product_pro: Option<String>,
```
In `from_env`, add:
```rust
            polar_access_token: std::env::var("POLAR_ACCESS_TOKEN").ok(),
            polar_webhook_secret: std::env::var("POLAR_WEBHOOK_SECRET").ok(),
            polar_base_url: std::env::var("POLAR_BASE_URL").unwrap_or_else(|_| "https://api.polar.sh".into()),
            polar_product_founding: std::env::var("POLAR_PRODUCT_FOUNDING").ok(),
            polar_product_standard: std::env::var("POLAR_PRODUCT_STANDARD").ok(),
            polar_product_pro: std::env::var("POLAR_PRODUCT_PRO").ok(),
```
Update the `Config { .. }` literal in `tests/health.rs` and every other test that builds `Config { .. }` to include the new fields (set `None`/defaults). SEARCH the crate for `Config {` and update ALL sites: `polar_access_token: None, polar_webhook_secret: None, polar_base_url: "https://api.polar.sh".into(), polar_product_founding: None, polar_product_standard: None, polar_product_pro: None`.

- [ ] **Step 2: Plan enum + mapping**

Create `control-plane/src/billing/mod.rs`:
```rust
pub mod plan;
pub mod polar;
pub mod routes;
pub mod store;
pub mod webhook;
pub mod webhook_sig;
```
(Create empty stubs `// later task` for `polar`, `routes`, `webhook`, `webhook_sig` now so the module compiles; Tasks 3–5 replace them.)

Create `control-plane/src/billing/plan.rs`:
```rust
//! The three plans + mapping a Polar product id back to a plan (via config).
use crate::config::Config;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Founding,
    Standard,
    Pro,
}

impl Plan {
    pub fn as_str(self) -> &'static str {
        match self {
            Plan::Founding => "founding",
            Plan::Standard => "standard",
            Plan::Pro => "pro",
        }
    }
    pub fn from_str(s: &str) -> Option<Plan> {
        match s {
            "founding" => Some(Plan::Founding),
            "standard" => Some(Plan::Standard),
            "pro" => Some(Plan::Pro),
            _ => None,
        }
    }
    pub fn is_founding(self) -> bool {
        matches!(self, Plan::Founding)
    }
    /// Map a Polar product id to a Plan using the configured product ids.
    pub fn from_polar_product(config: &Config, product_id: &str) -> Option<Plan> {
        if config.polar_product_founding.as_deref() == Some(product_id) {
            Some(Plan::Founding)
        } else if config.polar_product_standard.as_deref() == Some(product_id) {
            Some(Plan::Standard)
        } else if config.polar_product_pro.as_deref() == Some(product_id) {
            Some(Plan::Pro)
        } else {
            None
        }
    }
    /// The Polar product id to use when creating a checkout for this plan.
    pub fn polar_product_id(self, config: &Config) -> Option<String> {
        match self {
            Plan::Founding => config.polar_product_founding.clone(),
            Plan::Standard => config.polar_product_standard.clone(),
            Plan::Pro => config.polar_product_pro.clone(),
        }
    }
}
```

- [ ] **Step 3: License-gate store**

Create `control-plane/src/billing/store.rs`:
```rust
//! The license gate: read/write subscription state. `active_subscription` is the
//! single source of truth every other part of altkey reads.
use crate::billing::plan::Plan;
use crate::entities::{prelude::Subscription, subscription};
use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

/// The event shape we extract from a Polar subscription webhook (provider-agnostic
/// here so the webhook parser owns Polar's JSON shape).
pub struct PolarSubscriptionEvent {
    pub account_id: Uuid,
    pub polar_customer_id: Option<String>,
    pub polar_subscription_id: Option<String>,
    pub plan: Plan,
    pub status: String, // "active" | "trialing" | "past_due" | "canceled"
    pub current_period_end: Option<chrono::DateTime<Utc>>,
}

/// Return the account's subscription IF it is active/trialing and not past its period.
pub async fn active_subscription(db: &DatabaseConnection, account_id: Uuid) -> Result<Option<subscription::Model>> {
    let Some(s) = Subscription::find()
        .filter(subscription::Column::AccountId.eq(account_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let live = matches!(s.status.as_str(), "active" | "trialing");
    let unexpired = s.current_period_end.map(|e| e > Utc::now()).unwrap_or(true);
    Ok((live && unexpired).then_some(s))
}

/// Upsert (by account_id) the subscription from a Polar webhook event.
pub async fn upsert_from_polar(db: &DatabaseConnection, ev: &PolarSubscriptionEvent) -> Result<()> {
    let now = Utc::now();
    let existing = Subscription::find()
        .filter(subscription::Column::AccountId.eq(ev.account_id))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let mut am: subscription::ActiveModel = row.into();
            am.polar_customer_id = Set(ev.polar_customer_id.clone());
            am.polar_subscription_id = Set(ev.polar_subscription_id.clone());
            am.plan = Set(ev.plan.as_str().to_string());
            am.status = Set(ev.status.clone());
            am.current_period_end = Set(ev.current_period_end.map(|d| d.into()));
            am.is_founding = Set(ev.plan.is_founding());
            am.updated_at = Set(now.into());
            am.update(db).await?;
        }
        None => {
            subscription::ActiveModel {
                id: Set(Uuid::new_v4()),
                account_id: Set(ev.account_id),
                polar_customer_id: Set(ev.polar_customer_id.clone()),
                polar_subscription_id: Set(ev.polar_subscription_id.clone()),
                plan: Set(ev.plan.as_str().to_string()),
                status: Set(ev.status.clone()),
                current_period_end: Set(ev.current_period_end.map(|d| d.into())),
                is_founding: Set(ev.plan.is_founding()),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Declare billing + test the gate**

Add `pub mod billing;` to `control-plane/src/lib.rs`.

Create `control-plane/tests/billing_gate.rs`:
```rust
//! active_subscription returns the row only when live + unexpired; upsert updates in place.
use control_plane::billing::plan::Plan;
use control_plane::billing::store::{active_subscription, upsert_from_polar, PolarSubscriptionEvent};
use migration::MigratorTrait;
use sea_orm::Database;

#[tokio::test]
async fn gate_reflects_status_and_expiry() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let acct = uuid::Uuid::new_v4();

    // Active, unexpired → gate open.
    upsert_from_polar(&db, &PolarSubscriptionEvent {
        account_id: acct,
        polar_customer_id: Some("cus".into()),
        polar_subscription_id: Some("sub".into()),
        plan: Plan::Standard,
        status: "active".into(),
        current_period_end: Some(chrono::Utc::now() + chrono::Duration::days(30)),
    }).await.unwrap();
    assert!(active_subscription(&db, acct).await.unwrap().is_some());

    // Canceled → gate closed (upsert updates the same row).
    upsert_from_polar(&db, &PolarSubscriptionEvent {
        account_id: acct,
        polar_customer_id: Some("cus".into()),
        polar_subscription_id: Some("sub".into()),
        plan: Plan::Standard,
        status: "canceled".into(),
        current_period_end: Some(chrono::Utc::now() + chrono::Duration::days(30)),
    }).await.unwrap();
    assert!(active_subscription(&db, acct).await.unwrap().is_none());

    // Active but expired → gate closed.
    upsert_from_polar(&db, &PolarSubscriptionEvent {
        account_id: acct,
        polar_customer_id: Some("cus".into()),
        polar_subscription_id: Some("sub".into()),
        plan: Plan::Pro,
        status: "active".into(),
        current_period_end: Some(chrono::Utc::now() - chrono::Duration::days(1)),
    }).await.unwrap();
    assert!(active_subscription(&db, acct).await.unwrap().is_none());

    // Unknown account → None.
    assert!(active_subscription(&db, uuid::Uuid::new_v4()).await.unwrap().is_none());
}
```

- [ ] **Step 5: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test billing_gate`
Expected: PASS. Confirm `tests/health.rs` etc. still compile with the new `Config` fields.
```bash
git add control-plane/src/billing control-plane/src/config.rs control-plane/src/lib.rs control-plane/tests/billing_gate.rs control-plane/tests/health.rs
git commit -m "feat(control-plane): Plan mapping + subscription license-gate store"
```

---

## Task 3: Standard-Webhooks signature verification

**Files:**
- Modify: `control-plane/Cargo.toml` (add `hmac = "0.12"`, `subtle = "2"`; `base64`/`sha2`/`hex` already present)
- Replace: `control-plane/src/billing/webhook_sig.rs`
- Test: `control-plane/tests/billing_webhook_sig.rs`

- [ ] **Step 1: Add deps**

In `control-plane/Cargo.toml`: `hmac = "0.12"`, `subtle = "2"`. (`sha2`/`base64` already in deps from earlier tasks; add `sha2 = "0.10"` to control-plane deps if not present — it is via altkey-api but control-plane needs its own direct dep, so add it.)

- [ ] **Step 2: Implement Standard-Webhooks HMAC verify**

Polar signs webhooks with the Standard Webhooks spec (svix-style): the signed content is `"{id}.{timestamp}.{body}"`, HMAC-SHA256 with the secret (the secret is base64 after stripping an optional `whsec_` prefix), and the `webhook-signature` header is a space-separated list of `v1,<base64sig>` entries.

Replace `control-plane/src/billing/webhook_sig.rs`:
```rust
//! Standard-Webhooks (svix-style) signature verification, as Polar uses. The signed
//! payload is `{id}.{timestamp}.{body}`; the secret is base64 (optionally `whsec_`-
//! prefixed); the `webhook-signature` header holds space-separated `v1,<b64sig>`.
use anyhow::{anyhow, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub struct WebhookHeaders {
    pub id: String,
    pub timestamp: String,
    pub signature: String, // raw header value (may contain multiple "v1,..." entries)
}

/// Verify the signature over the raw body. Returns Ok(()) if any provided v1 sig matches.
pub fn verify(secret: &str, headers: &WebhookHeaders, body: &[u8]) -> Result<()> {
    let key_b64 = secret.strip_prefix("whsec_").unwrap_or(secret);
    let key = base64::engine::general_purpose::STANDARD
        .decode(key_b64)
        .map_err(|_| anyhow!("webhook secret not base64"))?;

    let signed = format!("{}.{}.{}", headers.id, headers.timestamp, String::from_utf8_lossy(body));
    let mut mac = HmacSha256::new_from_slice(&key).map_err(|_| anyhow!("bad hmac key"))?;
    mac.update(signed.as_bytes());
    let expected = mac.finalize().into_bytes();

    for part in headers.signature.split(' ') {
        // Each part is "v1,<base64sig>"; take the portion after the comma.
        let sig_b64 = part.split_once(',').map(|(_, s)| s).unwrap_or(part);
        if let Ok(provided) = base64::engine::general_purpose::STANDARD.decode(sig_b64) {
            if provided.len() == expected.len() && provided.ct_eq(&expected).into() {
                return Ok(());
            }
        }
    }
    Err(anyhow!("no matching webhook signature"))
}

/// Compute the header signature for a body — used by tests (and never in prod).
#[cfg(any(test, feature = "test-helpers"))]
pub fn sign(secret: &str, id: &str, timestamp: &str, body: &[u8]) -> String {
    let key_b64 = secret.strip_prefix("whsec_").unwrap_or(secret);
    let key = base64::engine::general_purpose::STANDARD.decode(key_b64).unwrap();
    let signed = format!("{}.{}.{}", id, timestamp, String::from_utf8_lossy(body));
    let mut mac = HmacSha256::new_from_slice(&key).unwrap();
    mac.update(signed.as_bytes());
    let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    format!("v1,{sig}")
}
```
Add a `test-helpers` feature to `control-plane/Cargo.toml` (`[features] test-helpers = []`) and a `[[test]]` entry isn't needed (the test can enable it). Simpler: gate `sign` on `#[cfg(any(test, feature = "test-helpers"))]` and have the test call it via the lib with the feature — OR just make `sign` always-available `pub` (it's harmless). RECOMMENDED: make `sign` unconditionally `pub` (drop the cfg) to avoid feature plumbing; it's a utility, not a risk.

RECONCILIATION: confirm Polar's exact header names — they are `webhook-id`, `webhook-timestamp`, `webhook-signature` (Standard Webhooks). If Polar uses a different scheme in the installed/sandbox version, adjust the signed-content format + header names to match Polar's docs. The OUTCOME: a body signed with `sign()` verifies with `verify()`, and a tampered body fails.

- [ ] **Step 3: Test**

Create `control-plane/tests/billing_webhook_sig.rs`:
```rust
//! A body signed with the shared secret verifies; tampering or wrong secret fails.
use control_plane::billing::webhook_sig::{sign, verify, WebhookHeaders};

fn secret() -> String {
    use base64::Engine;
    // 32 random-ish bytes base64'd, with the whsec_ prefix Polar uses.
    format!("whsec_{}", base64::engine::general_purpose::STANDARD.encode([7u8; 32]))
}

#[test]
fn valid_signature_verifies_and_tamper_fails() {
    let s = secret();
    let body = br#"{"type":"subscription.active","data":{}}"#;
    let sig = sign(&s, "msg_1", "1700000000", body);
    let h = WebhookHeaders { id: "msg_1".into(), timestamp: "1700000000".into(), signature: sig };

    assert!(verify(&s, &h, body).is_ok());

    // Tampered body fails.
    let tampered = br#"{"type":"subscription.canceled","data":{}}"#;
    assert!(verify(&s, &h, tampered).is_err());

    // Wrong secret fails.
    let other = format!("whsec_{}", base64::engine::general_purpose::STANDARD.encode([9u8; 32]));
    assert!(verify(&other, &h, body).is_err());
}
```

- [ ] **Step 4: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test billing_webhook_sig`
Expected: PASS.
```bash
git add control-plane/Cargo.toml control-plane/src/billing/webhook_sig.rs control-plane/tests/billing_webhook_sig.rs
git commit -m "feat(control-plane): Standard-Webhooks HMAC signature verification"
```

---

## Task 4: Polar webhook handler

**Files:**
- Replace: `control-plane/src/billing/webhook.rs`
- Modify: `control-plane/src/app.rs` (route), `control-plane/src/state.rs` (no change needed unless polar client used here)
- Test: `control-plane/tests/billing_webhook.rs`

- [ ] **Step 1: Webhook handler**

Replace `control-plane/src/billing/webhook.rs`:
```rust
//! POST /webhooks/polar — verify the Standard-Webhooks signature over the RAW body,
//! parse subscription events, and upsert the account's subscription. Our account id
//! travels in the checkout `metadata.account_id`, which Polar echoes on the
//! subscription object.
use crate::billing::plan::Plan;
use crate::billing::store::{upsert_from_polar, PolarSubscriptionEvent};
use crate::billing::webhook_sig::{verify, WebhookHeaders};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use chrono::DateTime;
use uuid::Uuid;

#[utoipa::path(post, path = "/webhooks/polar", tag = "billing",
    responses((status = 200, description = "Processed"), (status = 401, description = "Bad signature")))]
pub async fn polar_webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(secret) = state.config.polar_webhook_secret.clone() else {
        tracing::warn!("polar webhook received but POLAR_WEBHOOK_SECRET unset");
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let h = WebhookHeaders {
        id: header(&headers, "webhook-id"),
        timestamp: header(&headers, "webhook-timestamp"),
        signature: header(&headers, "webhook-signature"),
    };
    if verify(&secret, &h, &body).is_err() {
        return StatusCode::UNAUTHORIZED;
    }

    let Ok(ev) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    let typ = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if !typ.starts_with("subscription.") {
        return StatusCode::OK; // we only act on subscription.* events
    }
    match parse_subscription_event(&state, &ev) {
        Some(sub_ev) => {
            if let Err(e) = upsert_from_polar(&state.db, &sub_ev).await {
                tracing::error!("subscription upsert failed: {e:#}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            StatusCode::OK
        }
        None => {
            tracing::warn!("subscription event missing account_id/product mapping: {typ}");
            StatusCode::OK // ack so Polar doesn't retry forever; logged for triage
        }
    }
}

fn header(h: &HeaderMap, name: &str) -> String {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string()
}

/// Pull our PolarSubscriptionEvent out of Polar's webhook JSON. Maps Polar status
/// to our status vocabulary and product id → Plan.
fn parse_subscription_event(state: &AppState, ev: &serde_json::Value) -> Option<PolarSubscriptionEvent> {
    let data = ev.get("data")?;
    let account_id = data
        .get("metadata")
        .and_then(|m| m.get("account_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    let product_id = data.get("product_id").and_then(|v| v.as_str())
        .or_else(|| data.get("product").and_then(|p| p.get("id")).and_then(|v| v.as_str()))?;
    let plan = Plan::from_polar_product(&state.config, product_id)?;

    let polar_status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
    let status = map_status(polar_status).to_string();

    let current_period_end = data
        .get("current_period_end")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    Some(PolarSubscriptionEvent {
        account_id,
        polar_customer_id: data.get("customer_id").and_then(|v| v.as_str()).map(String::from),
        polar_subscription_id: data.get("id").and_then(|v| v.as_str()).map(String::from),
        plan,
        status,
        current_period_end,
    })
}

/// Normalize Polar's subscription status to our vocabulary.
fn map_status(polar: &str) -> &'static str {
    match polar {
        "active" => "active",
        "trialing" => "trialing",
        "past_due" | "unpaid" => "past_due",
        "canceled" | "revoked" | "incomplete_expired" => "canceled",
        _ => "active",
    }
}
```
RECONCILIATION: Polar's webhook JSON field names (`data.id`, `data.product_id`, `data.customer_id`, `data.status`, `data.current_period_end`, `data.metadata.account_id`) must match Polar's actual payload — check Polar's webhook docs/sandbox for a real `subscription.active`/`subscription.updated` payload and adjust the JSON pointers. The OUTCOME: a signed subscription webhook with our `account_id` in metadata upserts the right subscription row. Status/plan mapping is ours to define.

- [ ] **Step 2: Register the route**

In `control-plane/src/app.rs`, add to the OpenApiRouter chain: `.routes(routes!(crate::billing::webhook::polar_webhook))`. (It's documented via `#[utoipa::path]`, so it self-registers.)

- [ ] **Step 3: Test the handler end-to-end (signed body → subscription upserted)**

Create `control-plane/tests/billing_webhook.rs`:
```rust
//! A correctly-signed subscription.active webhook upserts the account's subscription;
//! a bad signature is rejected; an unmapped product is acked but does nothing.
use control_plane::billing::webhook::polar_webhook;
use control_plane::billing::webhook_sig::sign;
use control_plane::billing::store::active_subscription;
use control_plane::config::Config;
use control_plane::state::AppState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;

fn secret() -> String {
    use base64::Engine;
    format!("whsec_{}", base64::engine::general_purpose::STANDARD.encode([3u8; 32]))
}

async fn state() -> (AppState, sea_orm::DatabaseConnection) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let mut config = Config {
        database_url: "sqlite::memory:".into(),
        public_base_url: "http://localhost".into(),
        internal_service_secret: None,
        bind_addr: "127.0.0.1:0".into(),
        polar_access_token: None,
        polar_webhook_secret: Some(secret()),
        polar_base_url: "https://api.polar.sh".into(),
        polar_product_founding: None,
        polar_product_standard: Some("prod_standard".into()),
        polar_product_pro: None,
    };
    let st = AppState {
        db: db.clone(),
        config: config.clone(),
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: Arc::new(control_plane::billing::polar::FakePolarClient::default()),
    };
    let _ = &mut config;
    (st, db)
}

fn headers_for(secret: &str, body: &[u8]) -> HeaderMap {
    let sig = sign(secret, "msg_1", "1700000000", body);
    let mut h = HeaderMap::new();
    h.insert("webhook-id", "msg_1".parse().unwrap());
    h.insert("webhook-timestamp", "1700000000".parse().unwrap());
    h.insert("webhook-signature", sig.parse().unwrap());
    h
}

#[tokio::test]
async fn signed_subscription_webhook_upserts() {
    let (st, db) = state().await;
    let acct = uuid::Uuid::new_v4();
    let body = format!(
        r#"{{"type":"subscription.active","data":{{"id":"sub_1","customer_id":"cus_1","product_id":"prod_standard","status":"active","current_period_end":"2030-01-01T00:00:00Z","metadata":{{"account_id":"{acct}"}}}}}}"#
    ).into_bytes();

    let code = polar_webhook(State(st.clone()), headers_for(&secret(), &body), Bytes::from(body.clone())).await;
    assert_eq!(code, axum::http::StatusCode::OK);
    assert!(active_subscription(&db, acct).await.unwrap().is_some(), "subscription should be active");

    // Bad signature → 401.
    let mut bad = headers_for(&secret(), &body);
    bad.insert("webhook-signature", "v1,AAAA".parse().unwrap());
    let code = polar_webhook(State(st), bad, Bytes::from(body)).await;
    assert_eq!(code, axum::http::StatusCode::UNAUTHORIZED);
}
```
(This test references `FakePolarClient` + the `polar` field on `AppState` — both land in Task 5. To keep tasks independently runnable, you MAY implement Task 5's `polar.rs` stub (the `FakePolarClient` + trait + the `polar` AppState field) BEFORE this test, or fold the AppState `polar` field addition into this task. RECOMMENDED: implement Task 5's `polar.rs` + the `AppState.polar` field first if this test can't compile without it — the tasks are adjacent. Either way, end this task with `billing_webhook` passing.)

- [ ] **Step 4: Run + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane --test billing_webhook`
Expected: PASS.
```bash
git add control-plane/src/billing/webhook.rs control-plane/src/app.rs control-plane/tests/billing_webhook.rs
git commit -m "feat(control-plane): Polar webhook handler → subscription upsert"
```

---

## Task 5: PolarClient + billing routes

**Files:**
- Replace: `control-plane/src/billing/polar.rs`, `control-plane/src/billing/routes.rs`
- Modify: `control-plane/src/state.rs` (add `polar`), `control-plane/src/app.rs` (routes + state), `control-plane/src/main.rs`
- Test: `control-plane/tests/billing_routes.rs`

- [ ] **Step 1: PolarClient trait + impls**

Replace `control-plane/src/billing/polar.rs`:
```rust
//! Polar API client (checkout + customer portal). The real impl calls Polar's REST
//! API; the fake returns canned URLs so handlers + the webhook test run offline.
use crate::billing::plan::Plan;
use crate::config::Config;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use uuid::Uuid;

#[async_trait::async_trait]
pub trait PolarClient: Send + Sync {
    /// Create a checkout for `plan`, embedding our `account_id` in metadata so the
    /// webhook can map the resulting subscription back to the account. Returns the URL.
    async fn create_checkout(&self, account_id: Uuid, plan: Plan, success_url: &str) -> Result<String>;
    /// Return a customer-portal URL for managing/canceling a subscription.
    async fn customer_portal_url(&self, polar_customer_id: &str) -> Result<String>;
}

/// Real Polar REST client.
pub struct HttpPolarClient {
    pub config: Config,
    pub http: reqwest::Client,
}

impl HttpPolarClient {
    pub fn new(config: Config) -> Self {
        Self { config, http: reqwest::Client::new() }
    }
}

#[async_trait::async_trait]
impl PolarClient for HttpPolarClient {
    async fn create_checkout(&self, account_id: Uuid, plan: Plan, success_url: &str) -> Result<String> {
        let token = self.config.polar_access_token.as_ref().ok_or_else(|| anyhow!("POLAR_ACCESS_TOKEN unset"))?;
        let product_id = plan.polar_product_id(&self.config).ok_or_else(|| anyhow!("no polar product id for plan {:?}", plan))?;
        let body = serde_json::json!({
            "products": [product_id],
            "success_url": success_url,
            "metadata": { "account_id": account_id.to_string() }
        });
        let resp: serde_json::Value = self.http
            .post(format!("{}/v1/checkouts/", self.config.polar_base_url))
            .bearer_auth(token)
            .json(&body)
            .send().await?
            .error_for_status()?
            .json().await?;
        resp.get("url").and_then(|v| v.as_str()).map(String::from)
            .ok_or_else(|| anyhow!("polar checkout response missing url: {resp}"))
    }

    async fn customer_portal_url(&self, polar_customer_id: &str) -> Result<String> {
        let token = self.config.polar_access_token.as_ref().ok_or_else(|| anyhow!("POLAR_ACCESS_TOKEN unset"))?;
        let resp: serde_json::Value = self.http
            .post(format!("{}/v1/customer-sessions/", self.config.polar_base_url))
            .bearer_auth(token)
            .json(&serde_json::json!({ "customer_id": polar_customer_id }))
            .send().await?
            .error_for_status()?
            .json().await?;
        resp.get("customer_portal_url").and_then(|v| v.as_str()).map(String::from)
            .ok_or_else(|| anyhow!("polar customer session missing portal url: {resp}"))
    }
}

/// Test fake: deterministic URLs, no network.
#[derive(Default)]
pub struct FakePolarClient;

#[async_trait::async_trait]
impl PolarClient for FakePolarClient {
    async fn create_checkout(&self, account_id: Uuid, plan: Plan, _success_url: &str) -> Result<String> {
        Ok(format!("https://polar.test/checkout/{}/{}", plan.as_str(), account_id))
    }
    async fn customer_portal_url(&self, polar_customer_id: &str) -> Result<String> {
        Ok(format!("https://polar.test/portal/{polar_customer_id}"))
    }
}

/// Build the configured client (real if a token is set, else the fake so dev boots).
pub fn from_config(config: &Config) -> Arc<dyn PolarClient> {
    if config.polar_access_token.is_some() {
        Arc::new(HttpPolarClient::new(config.clone()))
    } else {
        Arc::new(FakePolarClient)
    }
}
```
RECONCILIATION: Polar's checkout endpoint + field names (`/v1/checkouts/`, `products`, `success_url`, `metadata`, response `url`) and customer-session endpoint (`/v1/customer-sessions/`, response `customer_portal_url`) should match Polar's current API — verify against Polar docs and adjust paths/fields. The OUTCOME: `create_checkout` returns a redirectable URL with our account_id in metadata; `customer_portal_url` returns a portal link.

- [ ] **Step 2: Add `polar` to AppState + boot**

Modify `control-plane/src/state.rs` to add:
```rust
    pub polar: std::sync::Arc<dyn crate::billing::polar::PolarClient>,
```
In `main.rs`, build it: `polar: control_plane::billing::polar::from_config(&config)`. Update ALL test `AppState { .. }` sites to add `polar: std::sync::Arc::new(control_plane::billing::polar::FakePolarClient)` (auth_me, auth_magic_link, auth_oauth, health, billing_webhook).

- [ ] **Step 3: Billing routes**

Replace `control-plane/src/billing/routes.rs`:
```rust
//! /billing/checkout (start a Polar checkout), /billing/portal (manage), and
//! /billing/subscription (current state) — all session-authenticated.
use crate::auth::extract::CurrentAccount;
use crate::billing::plan::Plan;
use crate::billing::store::active_subscription;
use crate::entities::{prelude::Subscription, subscription};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CheckoutRequest {
    pub plan: Plan,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct UrlResponse {
    pub url: String,
}

#[utoipa::path(post, path = "/billing/checkout", tag = "billing",
    request_body = CheckoutRequest,
    responses((status = 200, body = UrlResponse), (status = 401, description = "Not signed in")))]
pub async fn checkout(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
    Json(body): Json<CheckoutRequest>,
) -> Result<Json<UrlResponse>, ApiError> {
    let success = format!("{}/dashboard?checkout=success", state.config.public_base_url);
    let url = state.polar.create_checkout(acct.id, body.plan, &success).await.map_err(ApiError::Internal)?;
    Ok(Json(UrlResponse { url }))
}

#[utoipa::path(post, path = "/billing/portal", tag = "billing",
    responses((status = 200, body = UrlResponse), (status = 401), (status = 404, description = "No subscription")))]
pub async fn portal(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Result<Json<UrlResponse>, ApiError> {
    let sub = Subscription::find()
        .filter(subscription::Column::AccountId.eq(acct.id))
        .one(&state.db).await?
        .ok_or(ApiError::NotFound)?;
    let cust = sub.polar_customer_id.ok_or(ApiError::NotFound)?;
    let url = state.polar.customer_portal_url(&cust).await.map_err(ApiError::Internal)?;
    Ok(Json(UrlResponse { url }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SubscriptionView {
    pub plan: Option<String>,
    pub status: String,
    pub active: bool,
    pub current_period_end: Option<String>,
    pub is_founding: bool,
}

#[utoipa::path(get, path = "/billing/subscription", tag = "billing",
    responses((status = 200, body = SubscriptionView), (status = 401)))]
pub async fn subscription(
    State(state): State<AppState>,
    CurrentAccount(acct): CurrentAccount,
) -> Result<Json<SubscriptionView>, ApiError> {
    let active = active_subscription(&state.db, acct.id).await?;
    let row = Subscription::find()
        .filter(subscription::Column::AccountId.eq(acct.id))
        .one(&state.db).await?;
    Ok(Json(match row {
        Some(s) => SubscriptionView {
            plan: Some(s.plan),
            status: s.status,
            active: active.is_some(),
            current_period_end: s.current_period_end.map(|d| d.to_rfc3339()),
            is_founding: s.is_founding,
        },
        None => SubscriptionView { plan: None, status: "none".into(), active: false, current_period_end: None, is_founding: false },
    }))
}
```

- [ ] **Step 4: Register routes**

In `control-plane/src/app.rs` add to the OpenApiRouter chain:
```rust
        .routes(routes!(crate::billing::routes::checkout))
        .routes(routes!(crate::billing::routes::portal))
        .routes(routes!(crate::billing::routes::subscription))
```

- [ ] **Step 5: Test billing routes (with the fake Polar + a session)**

Create `control-plane/tests/billing_routes.rs`:
```rust
//! Boot the app, sign in (seed a session), and hit /billing/checkout + /billing/subscription.
use control_plane::app;
use control_plane::auth::session;
use control_plane::billing::plan::Plan;
use control_plane::billing::store::{upsert_from_polar, PolarSubscriptionEvent};
use control_plane::config::Config;
use control_plane::entities::account;
use control_plane::state::AppState;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, Set};
use std::sync::Arc;

async fn boot() -> (String, sea_orm::DatabaseConnection, uuid::Uuid, String) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let config = Config {
        database_url: "sqlite::memory:".into(), public_base_url: "http://localhost".into(),
        internal_service_secret: None, bind_addr: "127.0.0.1:0".into(),
        polar_access_token: None, polar_webhook_secret: None, polar_base_url: "https://api.polar.sh".into(),
        polar_product_founding: None, polar_product_standard: Some("prod_standard".into()), polar_product_pro: None,
    };
    let state = AppState {
        db: db.clone(), config,
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: Arc::new(control_plane::billing::polar::FakePolarClient),
    };
    let id = uuid::Uuid::new_v4();
    account::ActiveModel { id: Set(id), email: Set("sen@example.com".into()), display_name: Set(None), status: Set("active".into()), created_at: Set(chrono::Utc::now().into()) }.insert(&db).await.unwrap();
    let token = session::issue(&db, id).await.unwrap();
    let appx = app::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, appx).await.unwrap() });
    (format!("http://{addr}"), db, id, token)
}

#[tokio::test]
async fn checkout_and_subscription_views() {
    let (base, db, acct, token) = boot().await;
    let client = reqwest::Client::new();

    // checkout returns a (fake) Polar URL embedding the account id.
    let r = client.post(format!("{base}/billing/checkout"))
        .header("Cookie", format!("altkey_session={token}"))
        .json(&serde_json::json!({ "plan": "standard" }))
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert!(v["url"].as_str().unwrap().contains(&acct.to_string()));

    // No subscription yet → active=false.
    let r = client.get(format!("{base}/billing/subscription"))
        .header("Cookie", format!("altkey_session={token}")).send().await.unwrap();
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["active"], false);

    // After a webhook-style upsert → active=true.
    upsert_from_polar(&db, &PolarSubscriptionEvent {
        account_id: acct, polar_customer_id: Some("cus".into()), polar_subscription_id: Some("sub".into()),
        plan: Plan::Standard, status: "active".into(),
        current_period_end: Some(chrono::Utc::now() + chrono::Duration::days(30)),
    }).await.unwrap();
    let r = client.get(format!("{base}/billing/subscription"))
        .header("Cookie", format!("altkey_session={token}")).send().await.unwrap();
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["active"], true);
    assert_eq!(v["plan"], "standard");

    // /billing/subscription is in the served contract.
    let doc: serde_json::Value = client.get(format!("{base}/api-docs/openapi.json")).send().await.unwrap().json().await.unwrap();
    assert!(doc["paths"]["/billing/subscription"].is_object());
}
```

- [ ] **Step 6: Build + run all + commit**

Run: `cd "C:/Users/gsent/Desktop/altkey" && cargo test -p control-plane`
Expected: ALL control-plane tests pass (incl. billing_entity, billing_gate, billing_webhook_sig, billing_webhook, billing_routes).
Run: `cargo test --workspace` — all green.
```bash
git add control-plane/src/billing control-plane/src/state.rs control-plane/src/app.rs control-plane/src/main.rs control-plane/tests
git commit -m "feat(control-plane): PolarClient + /billing checkout/portal/subscription routes"
```

---

## Self-Review

**Spec coverage (3c slice):** Implements the spec's "Billing (Polar)" + "Endpoint key + license lifecycle (subscription side)": `subscription` table (data model) + the license-gate helper `active_subscription` (Task 2) that 3d reads; Polar checkout + customer portal via a `PolarClient` trait (Task 5); the signature-verified webhook that drives subscription state (Tasks 3–4); `/billing/checkout|portal|subscription` (Task 5). Founding/standard/pro plans + product-id mapping (Task 2). Out of scope: usage-based throughput enforcement (3e), the React billing UI (3f).

**Placeholder scan:** No "TBD". The intentional reconciliation points (Polar webhook JSON field names, checkout/customer-session endpoint shapes, Standard-Webhooks header names) carry the required OUTCOME + how to verify against Polar's docs — they are real adaptation instructions, since there's no Polar Rust SDK to pin exact shapes. The `account_id`-in-metadata round-trip (checkout sets it, webhook reads it) is the load-bearing contract and is fully specified.

**Type consistency:** `Plan` (founding/standard/pro) is the single enum across plan.rs, store.rs, polar.rs, routes.rs, webhook.rs, and tests. `PolarSubscriptionEvent` fields match between store.rs and webhook.rs. `active_subscription(db, account_id)` + `upsert_from_polar(db, &ev)` signatures are stable across store.rs, webhook.rs, routes.rs, and every test. `AppState` gains `polar` in Task 5; ALL construction sites (main.rs + all tests) are updated in that task. `Config`'s new Polar fields are added in Task 2 and every `Config { .. }` site updated there.

**Security:** webhook signature verified (constant-time compare via `subtle`) over the RAW body BEFORE parsing; unset webhook secret → 503 (fail closed, never silently accept); the license gate fails closed (no row / canceled / expired → not active). Card data never touches altkey (Polar is MoR). Carry-forward (from 3b review, still pending): verified-email account-takeover gate must land before real OAuth creds — unrelated to 3c but tracked.

**Cross-task ordering:** Task 4's webhook test references Task 5's `FakePolarClient` + the `AppState.polar` field; the plan flags this and recommends implementing Task 5's `polar.rs` + the AppState field before Task 4's test (the two tasks are adjacent and may be done together). Each task ends with its tests green and the crate compiling.
