# altkey Control Plane — Design Spec (Plan 3 of 4)

**Date:** 2026-06-01
**Status:** Approved design, ready for implementation planning
**Parent spec:** `2026-05-31-altkey-tunnel-design.md` (the 4-component model; this is sub-service #4, "altkey-cloud — the hosted brain")
**Builds on:** Plan 1 (transparent mode, merged), Plan 2 (the tunnel, merged)

**Goal:** Build altkey-cloud — the hosted control plane that makes altkey a sellable
product. It owns accounts, billing (Polar), the handle/agent/key registry, and is the
**single authority** that validates every endpoint key and every tunnel. It turns the two
Plan 2 stubs (`relay::validate()` → always-true; `engine::require_key` → local store) into
real, account-backed checks — **without ever holding a provider token or seeing request
plaintext** (the survival principle from the parent spec is preserved).

**One-line model:** *the cloud sells reachability + account + control, never access to AI.*

---

## Scope

**In scope (full control plane):** accounts + multi-provider login, Polar subscription
billing, the handle/agent/endpoint-key registry, the agent/relay-facing validation API,
usage metering + dashboard analytics, adapter catalog/delivery, and the React dashboard.

**Out of scope (future):** teams/SSO/audit log, time-limited trials, custom apex domains
(`api.you.com`), the desktop app (Plan 4), the ACME pipeline for publicly-trusted
`*.altkey.app` certs (Plan 2 ships a self-signed `HandleCert`; ACME slots into that trait
later as deploy infra).

---

## Decisions (locked during brainstorming)

| Area | Decision |
|---|---|
| Backend | Rust + **axum**, new `control-plane/` crate in the workspace |
| Data | **SeaORM** entities + migrations; **Postgres in prod, SQLite in dev/tests** (portable migrations) |
| API contract | **OpenAPI/Swagger** via `utoipa` + `utoipa-swagger-ui`, served from the API |
| Frontend | **React** (Vite + TS); models + API client **generated from the Swagger** (`@hey-api/openapi-ts`) |
| Contract rule | **Contract-first:** if React needs a model/endpoint not in the Swagger, codegen **halts and reports it missing** — never invent endpoints |
| Account auth | OAuth: **Google, Microsoft, Apple, GitHub** + **email magic link**; session-cookie based |
| Agent identity | **Agent/pairing token** (`ak_agent_…`) minted in the dashboard, pasted into the agent; presented to relay + validation API |
| Billing | **Polar** (merchant-of-record); $10 founding / $15 standard / $25 pro |
| Hosting | **Host-agnostic / 12-factor** — relay + control-plane as independent binaries, React a static bundle; host chosen at deploy time |

---

## Architecture

**New crate `control-plane/`** (axum) is altkey-cloud's brain and single source of truth.
**New shared crate `altkey-api/`** holds the request/response types and the `ak_agent_` /
`ak_live_` token formats, so the **relay** and **engine** call the control plane against
typed contracts instead of hand-rolled JSON.

### How it wires the two Plan 2 stubs into reality

```
relay/src/agent_conn.rs::validate(handle, token)     // Plan 2: returns true
   ──▶ POST {cp}/internal/agent/authorize { handle, agent_token }   (+ service secret)
       ↳ control plane: token valid & active? owns handle? sub active?
       ↳ 200 { plan, limits }  |  403

engine/src/auth.rs::require_key(headers)             // Plan 2: store::key_exists()
   ──▶ POST {cp}/internal/key/validate { ak_live_key, agent_token }
       ↳ control plane authority → { valid, sub_active, plan }
       ↳ agent CACHES (short TTL) + bounded OFFLINE-GRACE, then fails closed
```

### The three trust tiers stay distinct (parent spec, locked)

| Tier | Between | Credential | Where validated |
|---|---|---|---|
| Account session | browser ↔ cloud | session cookie | control plane |
| **Agent token** | one machine ↔ cloud | `ak_agent_…` (hashed at rest) | control plane (`authorize`, `key/validate`) |
| Endpoint key | calling app ↔ agent | `ak_live_…` | the **agent** (it terminates TLS); cloud is the authority it asks |

The provider OAuth token **never** touches the cloud — unchanged from the parent spec.

### Components inside `control-plane/`

`auth` (OAuth + magic-link + sessions) · `billing` (Polar checkout + webhooks) ·
`registry` (accounts, handles, agents, keys) · `internal` (agent/relay-facing validation +
usage ingest + adapter delivery) · `usage` (metering rollups) · `entities` + `migration`
(SeaORM) · `openapi` (utoipa wiring + Swagger UI).

---

## Data model (SeaORM entities)

**Conventions:** UUID PKs; `timestamptz` (PG) / text (SQLite) timestamps; **string-valued
enums** (not PG-native, so migrations stay portable to SQLite); `Json` → jsonb (PG) / text
(SQLite); **every secret stored SHA-256 hashed**, only a display prefix kept. Migrations are
written DB-agnostic and run on both backends.

### Identity & auth
- **`account`** — `id`, `email` (unique, lowercased), `display_name?`, `status` (active/suspended), `created_at`. The identity key.
- **`identity`** — linked OAuth provider. `id`, `account_id→account`, `provider` (google/microsoft/apple/github), `provider_user_id`, `email_at_provider`, `created_at`. Unique `(provider, provider_user_id)`. One account → many identities (matched by email).
- **`session`** — `id`, `account_id`, `token_hash`, `created_at`, `expires_at`, `last_seen_at`, `ip?`, `user_agent?`. Cookie carries the opaque token (httpOnly/secure/SameSite); only the hash is stored.
- **`magic_link`** — `id`, `email`, `token_hash`, `expires_at`, `consumed_at?`. ~15-min TTL, single-use.
- **`oauth_flow`** — transient CSRF/PKCE state. `state`, `provider`, `pkce_verifier`, `return_to?`, `expires_at`.

### Billing
- **`subscription`** — `id`, `account_id`, `polar_customer_id`, `polar_subscription_id`, `plan` (founding/standard/pro), `status` (active/trialing/past_due/canceled), `current_period_end`, `is_founding` (bool), `created_at`, `updated_at`. **This row is the license gate.** Updated by Polar webhooks.

### Registry
- **`handle`** — `id`, `account_id`, `name` (globally unique, lowercased, DNS-safe — the `<handle>` in `<handle>.altkey.app`), `status` (active/revoked), `created_at`.
- **`agent`** — a paired machine. `id`, `account_id`, `handle_id→handle`, `name` (user label), `agent_token_hash`, `token_prefix`, `status` (active/revoked), `created_at`, `last_seen_at?`. **Many agents may point at one handle** (Pro failover); MVP is 1:1.
- **`endpoint_key`** — `id`, `account_id`, `agent_id?` (optional machine scope), `key_hash`, `key_prefix`, `name`, `created_at`, `last_used_at?`, `revoked_at?`.

### Usage & analytics
- **`usage_record`** — per-request event from agents. `id`, `account_id`, `agent_id`, `handle_id`, `key_id?`, `ts`, `provider`, `model`, `prompt_tokens`, `completion_tokens`, `total_tokens`, `tunnel_bytes?`, `tool?`. Append-only.
- **`usage_rollup`** — `account_id`, `period` (hour/day), `model?`, `tool?`, `provider?`, `sum_requests`, `sum_tokens`, `sum_bytes`. Derived from `usage_record`.

### Adapters
- **`adapter`** — catalog entry. `id`, `slug` (unique), `name`, `description`, `version`, `target_tool`, `manifest` (Json), `published_at`. Agent fetches manifests via the delivery endpoint; "apply locally" stays an agent concern.

**Throughput cap:** `authorize` returns `plan` + `limits` (concurrency + rate); the relay
enforces them **in memory** per tunnel (rolling window) — no extra table for MVP.
`usage_rollup` + `subscription.plan` supply the inputs; persistent counters are a future
refinement.

---

## API surface (OpenAPI)

Auth contexts: **🌐 session** (browser cookie) · **🔑 agent-token** (`ak_agent_`) ·
**🛠 internal service secret** (relay ↔ control-plane). Everything except internal/webhook
routes is in the Swagger the React app generates from.

### Auth — issues a session cookie
- `GET /auth/{provider}/start` — provider ∈ {google,microsoft,apple,github} → redirect (stores `oauth_flow`)
- `GET /auth/{provider}/callback` → upsert `account`+`identity` (by email) → session cookie → redirect
- `POST /auth/magic-link/request` `{email}` → emails a one-time link
- `GET /auth/magic-link/consume?token=…` → session cookie → redirect
- `POST /auth/logout` · `GET /me` → current account + sub status

### Billing (Polar)
- `POST /billing/checkout` `{plan}` 🌐 → Polar checkout URL
- `POST /billing/portal` 🌐 → Polar customer-portal URL
- `GET /billing/subscription` 🌐 → current `subscription`
- `POST /webhooks/polar` — **signature-verified**, no session → updates `subscription`

### Registry 🌐
- Handles: `GET /handles` · `GET /handles/availability?name=` · `POST /handles` `{name}` · `DELETE /handles/{id}`
- Agents: `GET /agents` · `POST /agents` `{handle_id,name}` → **returns `ak_agent_` once** · `DELETE /agents/{id}`
- Keys: `GET /keys` · `POST /keys` `{name, agent_id?}` → **returns `ak_live_` once** · `DELETE /keys/{id}`

### Internal — agent/relay facing 🔑🛠 (NOT in the React contract)
- `POST /internal/agent/authorize` `{handle, agent_token}` 🛠+🔑 → `{ok, account_id, plan, limits}` — **relay at tunnel connect** (replaces `validate()`)
- `POST /internal/key/validate` `{ak_live_key, agent_token}` 🔑 → `{valid, sub_active, plan}` — **agent per request**, cached + offline-grace (replaces `store::key_exists`)
- `POST /internal/usage` `{agent_token, records[]}` 🔑 — batched ingest
- `POST /internal/agent/heartbeat` `{agent_token}` 🔑 → updates `last_seen`
- `GET /internal/adapters` · `GET /internal/adapters/{slug}` 🔑 — manifest delivery

### Usage & adapters (dashboard) 🌐
- `GET /usage/summary?range=` → rollup series · `GET /usage/records?cursor=` → recent events (paginated)
- `GET /adapters` → catalog

### Contract surface
- `GET /api-docs/openapi.json` (utoipa) + `GET /swagger-ui` → the React codegen source.
  **If React references something absent from the spec, codegen fails loudly and we stop.**

---

## Flows

**OAuth + magic-link + sessions.** Each provider uses Authorization-Code + PKCE: `start`
stores `oauth_flow` (state+verifier) and redirects; `callback` exchanges the code, reads the
provider's email, **upserts `account` by email and links an `identity`** (Google-then-GitHub
on one email = one account). **Apple** is the awkward case — its "client secret" is an ES256
JWT signed with a `.p8` key that must be minted/rotated, and the name is only returned on
first consent; it gets its own task. Magic-link: `request` stores a hashed single-use token
(15-min TTL) and sends it via a pluggable **`EmailSender`** trait (Resend/SMTP impl in prod,
a capturing impl in tests). All flows end by minting a `session` (opaque token → cookie; hash
stored).

**Polar billing lifecycle.** `checkout` opens a Polar checkout for the chosen plan and
returns its URL. Polar (merchant-of-record — handles tax/payouts, good for a pseudonymous
operator) calls `POST /webhooks/polar`; we **verify the signature**, then on
`subscription.created/updated/canceled` upsert the `subscription` row — the license gate.
`portal` deep-links to Polar for cancel/upgrade. No card data ever touches us.

**Validation wiring (the heart of it).**
- *Relay → `authorize`*: at tunnel connect the relay sends `{handle, agent_token}` + its
  internal service secret. Control plane checks token valid & active, owns the handle, **sub
  active** → returns `{plan, limits}`. Relay caches briefly, enforces concurrency/throughput
  in memory, and **drops the tunnel** if a later re-check fails (lapse/revoke).
- *Agent → `key/validate`*: per request the agent checks the `ak_live_` key, **caches**
  results with a short TTL (≈60 s), and keeps a **bounded offline-grace window** (≈72 h) if
  the control plane is unreachable, then **fails closed** — a control-plane blip never
  instantly kills every user's local proxy. `engine/src/auth.rs::require_key` gains a
  **`KeyValidator`** trait: `ControlPlaneValidator` (prod) vs the existing local store
  (dev/`ALTKEY_TRANSPARENT`), so existing engine tests stay green.

**Usage metering.** The agent batches `usage_record`s (model, tokens, tunnel bytes, tool)
and POSTs to `/internal/usage` on a timer; a rollup job aggregates into `usage_rollup` for the
dashboard charts and the fair-cap inputs. Best-effort: metering failure never blocks a
request.

---

## React frontend

Vite + TypeScript + **`@hey-api/openapi-ts`** generating typed models + client from
`/api-docs/openapi.json`; reuses the warm-dark/brass design language. A `pnpm gen` step (and
a CI check) regenerates the client; **if a referenced model/endpoint is absent from the spec,
codegen fails and the build stops** — the contract-first rule enforced mechanically. The
dashboard surface: sign up / log in → subscribe → claim a handle → pair a machine (copy the
`ak_agent_` token) → mint an `ak_live_` key → view status + usage.

---

## Error handling

Wire codes from the parent spec: `503 agent_offline` (machine off), `402/403` on
lapsed-sub / revoked-key (relay rejects before forwarding; agent fails closed after grace),
`provider_auth_expired` (reconnect prompt), tunnel-drop → agent auto-reconnect with backoff
while the relay briefly holds the handle. Control-plane unreachable → cached validation +
bounded offline grace; never a global outage of users' local proxies from a control-plane
blip.

---

## Security

- Secrets (sessions, agent tokens, endpoint keys, magic links) **hashed at rest**; only
  display prefixes kept.
- Session cookies httpOnly / secure / SameSite; OAuth **PKCE + state**; Polar webhook
  **signature verified**.
- Internal endpoints require the **service secret** (relay) and/or a valid **agent token**
  (agent); auth + validation routes rate-limited.
- The provider OAuth token still **never** reaches the cloud (survival principle).
- Closed-source trust posture unchanged: relay passthrough crypto (can't read), the cloud
  sells reachability + account, never inference.

---

## Testing

- **DB:** SeaORM against **SQLite in-memory** per test (zero DB service in CI).
- **Auth:** OAuth flows with a fake provider; magic-link with a capturing `EmailSender`;
  session issue/expiry.
- **Billing:** `POST /webhooks/polar` with signed fixtures → `subscription` transitions
  (active → past_due → canceled).
- **Validation:** truth-table for `authorize` / `key/validate` (valid / expired / revoked /
  lapsed-sub / wrong-handle); **offline-grace** simulated with a fake clock (within grace →
  serve; past grace → fail closed).
- **Integration:** `relay → /internal/agent/authorize → control-plane` replacing the Plan 2
  stub; assert reject on lapsed sub / wrong handle.
- **Frontend:** codegen smoke test — spec → generated client compiles; contract-first halt
  fires when a referenced symbol is missing.

---

## Deployment (host-agnostic / 12-factor)

Config via env: `DATABASE_URL`, per-provider OAuth client IDs/secrets (+ Apple `.p8`),
`POLAR_*`, `EMAIL_*`, `INTERNAL_SERVICE_SECRET`, `PUBLIC_BASE_URL`. `control-plane` and
`relay` are independent binaries; React builds to a static bundle (served by the API or any
static host); Postgres in prod, SQLite in dev; **SeaORM migrations run on boot**. The relay
gains a `CONTROL_PLANE_URL` + `INTERNAL_SERVICE_SECRET`; the agent gains `ALTKEY_AGENT_TOKEN`
+ `CONTROL_PLANE_URL`. Host is chosen at deploy time.

---

## Reused vs new

**Reused:** the `engine/` provider + translation stack (unchanged); the `relay/` SNI
passthrough (only `validate()` is rewired); `tunnel-proto` framing; the warm-dark/brass
design language from the landing page.

**New:** `control-plane/` crate (auth, billing, registry, internal API, usage, entities,
migration, openapi); `altkey-api/` shared contract crate; the React dashboard; the
`KeyValidator` trait in `engine/auth.rs`; the relay's HTTP call to `authorize`.
