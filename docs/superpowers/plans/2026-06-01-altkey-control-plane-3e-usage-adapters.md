# altkey Control Plane 3e — Usage & Adapters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Meter usage and ship adapters. The agent batches per-request usage to the control plane; the control plane rolls it up for the dashboard; users see usage analytics; and an adapter catalog is served (browse in the dashboard, fetched by the agent).

**Architecture:** Adds `usage_record` (append-only events), `usage_rollup` (aggregates), and `adapter` (catalog) tables. `POST /internal/usage` ingests agent-token-authenticated batches. A `rollup` pass aggregates records into per-account/day/model rollups. Session-gated `/usage/summary` + `/usage/records` read for the dashboard. `GET /adapters` (catalog) + `GET /internal/adapters[/{slug}]` (agent delivery). A best-effort engine usage reporter posts batches when configured. Metering never blocks a request.

**Tech Stack:** Rust, axum 0.7, SeaORM 1.1.20, the existing `altkey-api` DTOs, utoipa-axum. Reuses the internal-auth guards from 3d.

**Branch:** `feat/control-plane-3e` off `dev`.

---

## File Structure

| File | Responsibility |
|---|---|
| `altkey-api/src/dto.rs` | add `UsageRecordDto`, `UsageBatch` (modify) |
| `control-plane/migration/src/m20260601_000005_create_usage_adapter.rs` | usage_record, usage_rollup, adapter tables |
| `control-plane/migration/src/lib.rs` | register migration #5 (modify) |
| `control-plane/src/entities/{usage_record,usage_rollup,adapter}.rs` + mod/prelude | entities |
| `control-plane/src/usage/mod.rs`, `store.rs`, `rollup.rs`, `routes.rs` | ingest + rollup + dashboard reads |
| `control-plane/src/internal/routes.rs` | add `ingest_usage` (modify) |
| `control-plane/src/adapters/mod.rs`, `store.rs`, `routes.rs` | catalog + delivery |
| `control-plane/src/app.rs`, `lib.rs` | register routes/modules (modify) |
| `engine/src/usage.rs` | best-effort batch reporter (new) |
| tests | usage ingest+rollup, dashboard reads, adapter catalog |

**Shared DTOs (altkey-api/src/dto.rs) — agent → control plane:**
```rust
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct UsageRecordDto {
    pub ts: String,          // RFC3339
    pub provider: String,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub tunnel_bytes: i64,
    pub tool: Option<String>,
    pub key_prefix: Option<String>, // which ak_live_ (prefix only)
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct UsageBatch { pub agent_token: String, pub records: Vec<UsageRecordDto> }
```

---

## Task 1: DTOs + usage/adapter migrations + entities

**Files:**
- Modify: `altkey-api/src/dto.rs`
- Create: `control-plane/migration/src/m20260601_000005_create_usage_adapter.rs`; modify `migration/src/lib.rs`
- Create: `control-plane/src/entities/{usage_record,usage_rollup,adapter}.rs`; modify `entities/mod.rs`, `prelude.rs`
- Test: `control-plane/tests/usage_entities.rs`

- [ ] **Step 1: DTOs** — add `UsageRecordDto` + `UsageBatch` (above) to `altkey-api/src/dto.rs`.

- [ ] **Step 2: Migration** — create `m20260601_000005_create_usage_adapter.rs`:
```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum UsageRecord { Table, Id, AccountId, AgentId, KeyPrefix, Ts, Provider, Model, PromptTokens, CompletionTokens, TotalTokens, TunnelBytes, Tool }
#[derive(DeriveIden)]
enum UsageRollup { Table, Id, AccountId, Period, Model, Tool, Provider, SumRequests, SumTokens, SumBytes }
#[derive(DeriveIden)]
enum Adapter { Table, Id, Slug, Name, Description, Version, TargetTool, Manifest, PublishedAt }

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(Table::create().table(UsageRecord::Table).if_not_exists()
            .col(ColumnDef::new(UsageRecord::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(UsageRecord::AccountId).uuid().not_null())
            .col(ColumnDef::new(UsageRecord::AgentId).uuid().null())
            .col(ColumnDef::new(UsageRecord::KeyPrefix).string().null())
            .col(ColumnDef::new(UsageRecord::Ts).timestamp_with_time_zone().not_null())
            .col(ColumnDef::new(UsageRecord::Provider).string().not_null())
            .col(ColumnDef::new(UsageRecord::Model).string().not_null())
            .col(ColumnDef::new(UsageRecord::PromptTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::CompletionTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::TotalTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::TunnelBytes).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::Tool).string().null())
            .to_owned()).await?;
        m.create_index(Index::create().name("idx_usage_record_account_ts").table(UsageRecord::Table).col(UsageRecord::AccountId).col(UsageRecord::Ts).to_owned()).await?;

        m.create_table(Table::create().table(UsageRollup::Table).if_not_exists()
            .col(ColumnDef::new(UsageRollup::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(UsageRollup::AccountId).uuid().not_null())
            .col(ColumnDef::new(UsageRollup::Period).string().not_null()) // "YYYY-MM-DD"
            .col(ColumnDef::new(UsageRollup::Model).string().null())
            .col(ColumnDef::new(UsageRollup::Tool).string().null())
            .col(ColumnDef::new(UsageRollup::Provider).string().null())
            .col(ColumnDef::new(UsageRollup::SumRequests).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRollup::SumTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRollup::SumBytes).big_integer().not_null().default(0))
            .to_owned()).await?;
        m.create_index(Index::create().name("idx_usage_rollup_account_period").table(UsageRollup::Table).col(UsageRollup::AccountId).col(UsageRollup::Period).to_owned()).await?;

        m.create_table(Table::create().table(Adapter::Table).if_not_exists()
            .col(ColumnDef::new(Adapter::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(Adapter::Slug).string().not_null())
            .col(ColumnDef::new(Adapter::Name).string().not_null())
            .col(ColumnDef::new(Adapter::Description).string().not_null().default(""))
            .col(ColumnDef::new(Adapter::Version).string().not_null().default("1.0.0"))
            .col(ColumnDef::new(Adapter::TargetTool).string().null())
            .col(ColumnDef::new(Adapter::Manifest).json().not_null())
            .col(ColumnDef::new(Adapter::PublishedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .to_owned()).await?;
        m.create_index(Index::create().name("idx_adapter_slug_unique").table(Adapter::Table).col(Adapter::Slug).unique().to_owned()).await
    }
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(Adapter::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(UsageRollup::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(UsageRecord::Table).to_owned()).await
    }
}
```
Register as migration #5.

RECONCILIATION: `big_integer()` → i64 (`BigInt`); `json()` → `Json` type (jsonb on PG, text on SQLite — SeaORM `Json`). Entity types: `i64`, `serde_json::Value` for manifest. If `.json()` needs a feature, sea-orm already has `with-json`? add `with-json` to control-plane's sea-orm features if `Json` fails to compile.

- [ ] **Step 3: Entities** — create `usage_record.rs`, `usage_rollup.rs`, `adapter.rs` (i64 for the sums/tokens, `Option<Uuid>` agent_id, `serde_json::Value` manifest via `#[sea_orm(column_type = "Json")]` — reconcile to SeaORM 1.1.20). Add to mod/prelude (`UsageRecord`, `UsageRollup`, `Adapter`).

- [ ] **Step 4: Test + commit** — `usage_entities.rs` migrates SQLite + round-trips one row per table.
```bash
git checkout -b feat/control-plane-3e
git add control-plane/migration control-plane/src/entities altkey-api/src/dto.rs control-plane/tests/usage_entities.rs
git commit -m "feat(control-plane): usage_record/usage_rollup/adapter tables + DTOs"
```

---

## Task 2: Usage ingest (`/internal/usage`) + store

**Files:**
- Create: `control-plane/src/usage/mod.rs`, `store.rs`
- Modify: `control-plane/src/internal/routes.rs` (add `ingest_usage`), `app.rs`, `lib.rs`
- Test: `control-plane/tests/usage_ingest.rs`

- [ ] **Step 1: Store** — `control-plane/src/usage/mod.rs`:
```rust
pub mod rollup;
pub mod routes;
pub mod store;
```
(stub `rollup`/`routes` as `// later task`.)
`control-plane/src/usage/store.rs`:
```rust
//! Append usage records ingested from agents. Never fails a request — caller acks
//! regardless; bad rows are skipped.
use crate::entities::usage_record;
use altkey_api::dto::UsageRecordDto;
use anyhow::Result;
use chrono::DateTime;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use uuid::Uuid;

pub async fn insert_records(db: &DatabaseConnection, account_id: Uuid, agent_id: Option<Uuid>, records: &[UsageRecordDto]) -> Result<usize> {
    let mut n = 0;
    for r in records {
        let ts = DateTime::parse_from_rfc3339(&r.ts).map(|d| d.with_timezone(&chrono::Utc)).unwrap_or_else(|_| chrono::Utc::now());
        let ok = usage_record::ActiveModel {
            id: Set(Uuid::new_v4()), account_id: Set(account_id), agent_id: Set(agent_id),
            key_prefix: Set(r.key_prefix.clone()), ts: Set(ts.into()),
            provider: Set(r.provider.clone()), model: Set(r.model.clone()),
            prompt_tokens: Set(r.prompt_tokens), completion_tokens: Set(r.completion_tokens),
            total_tokens: Set(r.total_tokens), tunnel_bytes: Set(r.tunnel_bytes), tool: Set(r.tool.clone()),
        }.insert(db).await;
        if ok.is_ok() { n += 1; }
    }
    Ok(n)
}
```

- [ ] **Step 2: Ingest endpoint** — in `control-plane/src/internal/routes.rs` add:
```rust
#[utoipa::path(post, path = "/internal/usage", tag = "internal", responses((status = 200, description = "Ingested")))]
pub async fn ingest_usage(State(state): State<AppState>, Json(batch): Json<altkey_api::dto::UsageBatch>) -> StatusCode {
    let Some(agent) = crate::internal::auth::agent_for_token(&state, &batch.agent_token).await else { return StatusCode::UNAUTHORIZED; };
    let _ = crate::usage::store::insert_records(&state.db, agent.account_id, Some(agent.id), &batch.records).await;
    StatusCode::OK
}
```
Register in `app.rs` via `.routes(routes!(crate::internal::routes::ingest_usage))`. Add `pub mod usage;` to lib.rs.

- [ ] **Step 3: Test + commit** — `usage_ingest.rs`: seed account+agent; POST a batch with the agent token → 200; assert records stored (query usage_record count). Bad agent token → 401.
```bash
git add control-plane/src/usage control-plane/src/internal/routes.rs control-plane/src/app.rs control-plane/src/lib.rs control-plane/tests/usage_ingest.rs
git commit -m "feat(control-plane): /internal/usage ingest + usage store"
```

---

## Task 3: Rollup aggregation

**Files:**
- Replace: `control-plane/src/usage/rollup.rs`
- Test: `control-plane/tests/usage_rollup.rs`

- [ ] **Step 1: Rollup** — `control-plane/src/usage/rollup.rs`:
```rust
//! Aggregate raw usage_record rows into per-account/day/model usage_rollup rows.
//! Idempotent: clears an account's rollups for the affected days then rewrites them.
//! Simple in-Rust aggregation (portable across SQLite + Postgres).
use crate::entities::{prelude::*, usage_record, usage_rollup};
use anyhow::Result;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use uuid::Uuid;

/// Recompute rollups for one account from its raw records.
pub async fn rebuild_for_account(db: &DatabaseConnection, account_id: Uuid) -> Result<()> {
    let records = UsageRecord::find().filter(usage_record::Column::AccountId.eq(account_id)).all(db).await?;
    // key: (period_yyyy_mm_dd, model, tool, provider)
    let mut agg: HashMap<(String, String, String, String), (i64, i64, i64)> = HashMap::new();
    for r in &records {
        let period = r.ts.format("%Y-%m-%d").to_string();
        let key = (period, r.model.clone(), r.tool.clone().unwrap_or_default(), r.provider.clone());
        let e = agg.entry(key).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += r.total_tokens;
        e.2 += r.tunnel_bytes;
    }
    // Clear existing rollups for the account, then rewrite.
    UsageRollup::delete_many().filter(usage_rollup::Column::AccountId.eq(account_id)).exec(db).await?;
    for ((period, model, tool, provider), (reqs, tokens, bytes)) in agg {
        usage_rollup::ActiveModel {
            id: Set(Uuid::new_v4()), account_id: Set(account_id), period: Set(period),
            model: Set(if model.is_empty() { None } else { Some(model) }),
            tool: Set(if tool.is_empty() { None } else { Some(tool) }),
            provider: Set(if provider.is_empty() { None } else { Some(provider) }),
            sum_requests: Set(reqs), sum_tokens: Set(tokens), sum_bytes: Set(bytes),
        }.insert(db).await?;
    }
    Ok(())
}
```

- [ ] **Step 2: Test + commit** — `usage_rollup.rs`: insert several usage records across 2 days/2 models for one account; `rebuild_for_account`; assert rollup rows sum correctly (requests/tokens/bytes per day+model). Re-running is idempotent (counts don't double).
```bash
git add control-plane/src/usage/rollup.rs control-plane/tests/usage_rollup.rs
git commit -m "feat(control-plane): usage rollup aggregation (idempotent rebuild)"
```

---

## Task 4: Dashboard usage reads

**Files:**
- Replace: `control-plane/src/usage/routes.rs`; modify `app.rs`
- Test: `control-plane/tests/usage_routes.rs`

- [ ] **Step 1: Routes** — `control-plane/src/usage/routes.rs` (session-gated): `GET /usage/summary` → triggers `rebuild_for_account(acct.id)` then returns the account's rollups (Vec of `{period, model, tool, provider, sum_requests, sum_tokens, sum_bytes}`); `GET /usage/records?limit=` → recent raw records (newest first, capped at 200), scoped to the account, secrets-free. Both `#[utoipa::path]` + `ToSchema` response structs. Register both in app.rs.

- [ ] **Step 2: Test + commit** — `usage_routes.rs`: boot, seed account+session, ingest a few records (via store), GET /usage/summary → rollups present + correct totals; GET /usage/records → the records; assert `/usage/summary` in openapi.json; assert another account's session sees zero (isolation).
```bash
git add control-plane/src/usage/routes.rs control-plane/src/app.rs control-plane/tests/usage_routes.rs
git commit -m "feat(control-plane): /usage/summary + /usage/records dashboard reads"
```

---

## Task 5: Adapter catalog + delivery

**Files:**
- Create: `control-plane/src/adapters/mod.rs`, `store.rs`, `routes.rs`; modify `app.rs`, `lib.rs`
- Test: `control-plane/tests/adapters.rs`

- [ ] **Step 1: Store + seed** — `adapters/mod.rs` (`pub mod routes; pub mod store;`), `adapters/store.rs`: `list()` (all adapters), `get(slug)` (one), and `seed_defaults(db)` that inserts a couple of starter adapters (e.g. slug `openai-base-url`, name "OpenAI Base URL Shim", a small JSON manifest) if the table is empty — called at boot from `main.rs` after migrations (best-effort).

- [ ] **Step 2: Routes** — `adapters/routes.rs`:
  - `GET /adapters` (session-gated, catalog view) → list of `{slug, name, description, version, target_tool}`.
  - `GET /internal/adapters` (agent-token via query/body or a simple header? — keep it simple: agent-token in a `?agent_token=` query OR just public read since manifests aren't secret) → list with manifests.
  - `GET /internal/adapters/{slug}` → one adapter's manifest.
  Decision: adapter manifests are NOT secret (they describe public shims), so `/internal/adapters*` can be UNauthenticated read (simpler) — document that. `/adapters` (dashboard) stays session-gated for a consistent dashboard contract. Register all in app.rs (the two `/internal/adapters*` are plain `.route(...)` or `routes!()` — they're GET reads).

- [ ] **Step 3: lib + seed wiring** — `pub mod adapters;` in lib.rs; call `adapters::store::seed_defaults(&db).await.ok();` in `main.rs` after `run_migrations`.

- [ ] **Step 4: Test + commit** — `adapters.rs`: boot (seed runs), GET /internal/adapters → ≥1 adapter; GET /internal/adapters/{slug} → manifest; GET /adapters with a session → catalog; assert `/adapters` in openapi.json.
```bash
git add control-plane/src/adapters control-plane/src/app.rs control-plane/src/lib.rs control-plane/src/main.rs control-plane/tests/adapters.rs
git commit -m "feat(control-plane): adapter catalog + delivery endpoints + seed"
```

---

## Task 6: Engine best-effort usage reporter

**Files:**
- Create: `engine/src/usage.rs`; modify `engine/src/lib.rs`, `main.rs`, `config.rs`
- Test: `engine/tests/usage_reporter.rs`

- [ ] **Step 1: Reporter** — `engine/src/usage.rs`: a `UsageReporter` holding a `Mutex<Vec<UsageRecordDto>>` buffer + the control-plane URL + agent token. `record(dto)` pushes to the buffer (non-blocking). `flush()` drains the buffer and POSTs an `altkey_api::dto::UsageBatch` to `{CONTROL_PLANE_URL}/internal/usage`; on error, drops the batch (best-effort — never blocks or retries forever; log a warn). A background task (spawned in main.rs when configured) calls `flush()` every ~10s. When `CONTROL_PLANE_URL`/`ALTKEY_AGENT_TOKEN` unset, `record` is a no-op (or buffers without flushing). Keep it simple + non-blocking.
  Config: reuse `config::control_plane_url()` + `config::agent_token()` (added in 3d).
  NOTE: wiring `record(...)` into the actual request path (capturing real tokens/bytes) is a light touch — for THIS task, expose `UsageReporter` + a global accessor and call `record` from ONE place (e.g. after a successful `/v1/chat/completions`), capturing model + token counts if readily available from the response, else a minimal record (provider, model, ts, total_tokens=0). Don't over-engineer; the reporter mechanism + one call site is the deliverable. If token counts aren't easily available at the call site, record provider+model+ts with zeros and leave a `// TODO: thread real token counts` — the metering pipeline is what's being built.

- [ ] **Step 2: Test + commit** — `usage_reporter.rs`: stand up a stub axum server capturing posted `UsageBatch`; build a `UsageReporter` pointed at it; `record` two dtos; `flush`; assert the stub received a batch with 2 records and the agent token. Unconfigured reporter (no URL) → `flush` is a no-op (no panic).
```bash
git add engine/src/usage.rs engine/src/lib.rs engine/src/main.rs engine/src/config.rs engine/tests/usage_reporter.rs
git commit -m "feat(engine): best-effort usage reporter (batched, non-blocking)"
```

---

## Task 7: Full workspace green + clippy

- [ ] **Step 1:** `cd "C:/Users/gsent/Desktop/altkey" && cargo test --workspace` — all green; `cargo test -p altkey-relay --features test-helpers` — green.
- [ ] **Step 2:** `cargo clippy -p control-plane -p altkey-api -p altkey --all-targets 2>&1` — fix NEW warnings in 3e code.
- [ ] **Step 3:** Commit any fixups: `git commit -am "test(control-plane): 3e workspace green + clippy"` (skip if nothing).

---

## Self-Review

**Spec coverage (3e slice):** Implements "usage metering + dashboard analytics" + "adapter catalog/delivery": `usage_record`/`usage_rollup`/`adapter` tables; `/internal/usage` ingest (agent-token authed, never blocks); idempotent rollup; `/usage/summary` + `/usage/records` dashboard reads (account-scoped); `/adapters` catalog + `/internal/adapters[/{slug}]` delivery; a best-effort engine reporter. Out of scope: throughput-cap *enforcement* from usage (the relay already enforces limits in-memory from `authorize`'s `Limits`; persistent cap accounting is future); the React UI (3f).

**Placeholder scan:** The engine reporter's request-path call site (Task 6 Step 1) carries a documented "minimal record + TODO thread real token counts" — an explicit, scoped instruction (the pipeline is the deliverable, exact token capture is a refinement), not a vague gap. Adapter `/internal/adapters*` is intentionally unauthenticated (manifests aren't secret) — a stated decision, not an omission.

**Type consistency:** `UsageRecordDto`/`UsageBatch` (altkey-api/dto.rs) shared by the engine reporter + the control-plane ingest. `insert_records(db, account_id, agent_id, &[dto])` + `rebuild_for_account(db, account_id)` stable across store/rollup/routes/tests. Entity i64 columns match the DTO i64 fields. `agent_for_token` (3d) reused for ingest auth.

**Security:** ingest is agent-token authenticated (unknown token → 401, no write); usage reads are session-scoped to the account (no cross-account); metering failures never block a request (best-effort, acked regardless). Adapter manifests are public by design. Prior tracked follow-ups (relay open-without-CONTROL_PLANE_URL, verified-email gate, webhook freshness) are unaffected.
