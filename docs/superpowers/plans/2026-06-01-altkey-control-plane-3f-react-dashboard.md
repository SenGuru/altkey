# altkey Control Plane 3f — React Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The dashboard — a React (Vite + TypeScript) single-page app whose API models + client are **generated from the control-plane's OpenAPI spec** (contract-first: referencing an endpoint/model absent from the spec fails the build). Users: sign in (magic-link / OAuth) → subscribe → claim a handle → pair a machine (copy the `ak_agent_` token) → mint an `ak_live_` key → view usage + adapters.

**Architecture:** A new `web/` project (NOT a cargo crate). A tiny control-plane binary dumps the OpenAPI JSON to a file; `@hey-api/openapi-ts` generates a typed client from it; the React app consumes ONLY the generated client. Verification is build-based: codegen succeeds, `tsc --noEmit` passes, `bun run build` succeeds — the contract-first rule is enforced mechanically (an import of a non-generated symbol fails typecheck). Warm-dark + brass design language (matching the landing page).

**Tech Stack:** Bun (package manager + runner), Vite 5, React 18, TypeScript 5, `@hey-api/openapi-ts` (codegen), `@tanstack/react-query` (data), `react-router-dom` (routing), plain CSS (warm-dark/brass tokens). Backend served by the Rust control-plane (same origin in prod; Vite dev-proxy in dev).

**Branch:** `feat/control-plane-3f` off `dev`.

---

## File Structure

| File | Responsibility |
|---|---|
| `control-plane/src/bin/dump_openapi.rs` | prints `ApiDoc + merged routes` OpenAPI JSON to stdout |
| `web/package.json`, `vite.config.ts`, `tsconfig.json` | project + dev proxy to the control plane |
| `web/openapi.json` | the dumped spec (regenerated; codegen input) |
| `web/openapi-ts.config.ts` | @hey-api codegen config |
| `web/src/client/` (generated) | typed models + SDK (DO NOT hand-edit) |
| `web/src/lib/api.ts` | configure the generated client (base URL, credentials) |
| `web/src/lib/queries.ts` | react-query hooks wrapping the SDK |
| `web/src/app.tsx`, `main.tsx`, `routes.tsx` | shell + routing + auth guard |
| `web/src/pages/{Login,Dashboard,Billing,Handles,Machines,Keys,Usage,Adapters}.tsx` | pages |
| `web/src/components/*` | shared UI (SecretModal, Card, Button, Nav) |
| `web/src/styles/theme.css` | warm-dark/brass tokens |

---

## Task 1: OpenAPI dump bin + Vite/React scaffold + codegen

**Files:**
- Create: `control-plane/src/bin/dump_openapi.rs`
- Create: `web/` scaffold (package.json, vite.config.ts, tsconfig.json, index.html, src/main.tsx, src/app.tsx, openapi-ts.config.ts)

- [ ] **Step 1: OpenAPI dump binary**

Create `control-plane/src/bin/dump_openapi.rs`:
```rust
//! Print the control-plane OpenAPI document (including all router-merged paths) to
//! stdout. The web build pipes this into web/openapi.json for client codegen.
use control_plane::state::AppState;

#[tokio::main]
async fn main() {
    // Build the app to obtain the merged OpenAPI (paths self-register via OpenApiRouter).
    // We need an AppState to call app::build; use an in-memory SQLite that we DON'T migrate
    // (we only want the spec, not a live DB). If app::build requires a live connection,
    // connect to sqlite::memory:.
    let db = sea_orm::Database::connect("sqlite::memory:").await.expect("db");
    let config = control_plane::config::Config::from_env().expect("config");
    let state = AppState {
        db,
        config,
        email: std::sync::Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: std::sync::Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: std::sync::Arc::new(control_plane::billing::polar::FakePolarClient),
    };
    let spec = control_plane::app::openapi_json(state);
    println!("{spec}");
}
```
This needs a helper that returns the merged OpenAPI JSON. Add to `control-plane/src/app.rs`:
```rust
/// Build the router purely to extract the merged OpenAPI document as pretty JSON.
pub fn openapi_json(state: AppState) -> String {
    use utoipa::OpenApi;
    use utoipa_axum::router::OpenApiRouter;
    // Reuse the same route registration as `build` by factoring it — simplest: rebuild
    // the OpenApiRouter here mirroring `build`'s `.routes(...)` calls and return the api.
    let (_router, api) = router_parts(state);
    serde_json::to_string_pretty(&api).unwrap_or_default()
}
```
REFACTOR: extract the `OpenApiRouter::with_openapi(...).routes(...).routes(...)....split_for_parts()` chain from `build` into a private `fn router_parts(state) -> (axum::Router, utoipa::openapi::OpenApi)`, and have BOTH `build` (which then `.merge(SwaggerUi...).with_state`) and `openapi_json` call it. This guarantees the dumped spec == the served spec. Keep `build`'s behavior identical.

Verify the bin runs: `cd "C:/Users/gsent/Desktop/altkey" && cargo run -p control-plane --bin dump_openapi > web/openapi.json` produces a JSON file with `paths` including `/me`, `/billing/subscription`, `/handles`, `/usage/summary`, `/adapters`. (Create `web/` dir first.)

- [ ] **Step 2: Scaffold the web project**

Create `web/package.json`:
```json
{
  "name": "altkey-dashboard",
  "private": true,
  "type": "module",
  "scripts": {
    "gen:spec": "cd .. && cargo run -p control-plane --bin dump_openapi > web/openapi.json",
    "gen": "openapi-ts",
    "dev": "vite",
    "typecheck": "tsc --noEmit",
    "build": "tsc --noEmit && vite build"
  },
  "dependencies": {
    "@tanstack/react-query": "^5.59.0",
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "react-router-dom": "^6.26.0"
  },
  "devDependencies": {
    "@hey-api/openapi-ts": "^0.53.0",
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0"
  }
}
```
`web/openapi-ts.config.ts`:
```ts
import { defineConfig } from '@hey-api/openapi-ts';
export default defineConfig({
  input: './openapi.json',
  output: { path: './src/client', format: 'prettier' },
  plugins: ['@hey-api/client-fetch'],
});
```
`web/tsconfig.json` (standard Vite React strict config — target ES2020, jsx react-jsx, strict true, moduleResolution bundler, `noUnusedLocals` true so missing-symbol references surface).
`web/vite.config.ts`:
```ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      // dev: proxy API + auth + webhooks to the local control plane
      '/me': 'http://127.0.0.1:8080',
      '/auth': 'http://127.0.0.1:8080',
      '/billing': 'http://127.0.0.1:8080',
      '/handles': 'http://127.0.0.1:8080',
      '/agents': 'http://127.0.0.1:8080',
      '/keys': 'http://127.0.0.1:8080',
      '/usage': 'http://127.0.0.1:8080',
      '/adapters': 'http://127.0.0.1:8080',
    },
  },
});
```
`web/index.html` + `web/src/main.tsx` (mount React + QueryClientProvider + RouterProvider) + a minimal `web/src/app.tsx` placeholder.

`web/.gitignore`: `node_modules`, `dist`, `src/client` (generated — regenerated by `bun run gen`; OR commit it — DECISION: gitignore `src/client` and commit `openapi.json` so codegen is reproducible from the committed spec).

- [ ] **Step 3: Install + generate + typecheck**

Run (in `web/`):
```bash
cd "C:/Users/gsent/Desktop/altkey/web" && bun install && bun run gen && bun run typecheck
```
Expected: `bun install` ok; `bun run gen` produces `src/client/` (types + sdk + the fetch client); `tsc --noEmit` passes on the scaffold (the placeholder app imports nothing missing). If `@hey-api/openapi-ts` version/flags differ, reconcile to the installed version — the OUTCOME: a typed `src/client` is generated from `openapi.json` with an SDK function per documented operation (e.g. `getMe`, `postBillingCheckout`, `getHandles`, etc., names per @hey-api's operationId/path derivation).

- [ ] **Step 4: Commit**

```bash
git checkout -b feat/control-plane-3f
git add control-plane/src/bin/dump_openapi.rs control-plane/src/app.rs web/package.json web/bun.lockb web/vite.config.ts web/tsconfig.json web/openapi-ts.config.ts web/openapi.json web/index.html web/src/main.tsx web/src/app.tsx web/.gitignore
git commit -m "feat(web): OpenAPI dump bin + Vite/React scaffold + generated client"
```
(Confirm `cargo test -p control-plane` still passes after the app.rs refactor — `build` must behave identically.)

---

## Task 2: API client config + react-query hooks + app shell + auth guard

**Files:**
- Create: `web/src/lib/api.ts`, `web/src/lib/queries.ts`, `web/src/routes.tsx`, `web/src/components/Nav.tsx`
- Modify: `web/src/app.tsx`, `web/src/main.tsx`

- [ ] **Step 1: Configure the generated client** — `web/src/lib/api.ts`: import the generated client and set the base URL (same-origin in prod, `''` works with the Vite proxy) and `credentials: 'include'` so the session cookie rides along. (@hey-api client-fetch exposes a `client.setConfig({ baseUrl, credentials: 'include' })` or similar — reconcile to the generated client's config API.)

- [ ] **Step 2: react-query hooks** — `web/src/lib/queries.ts`: thin hooks wrapping generated SDK calls — `useMe()` (GET /me; 401 → null), `useSubscription()`, `useHandles()`, `useAgents()`, `useKeys()`, `useUsageSummary()`, `useAdapters()`, plus mutations `useCreateHandle`, `useCreateAgent`, `useCreateKey`, `useDeleteX`, `useCheckout`. Each calls ONLY generated SDK functions (contract-first — a typo'd/absent operation fails `tsc`).

- [ ] **Step 3: Shell + routing + guard** — `web/src/routes.tsx` (react-router): `/login` (public), everything else behind an auth guard that redirects to `/login` when `useMe()` is null. `Nav.tsx`: links to Dashboard / Handles / Machines / Keys / Usage / Adapters / Billing + a logout button (POST /auth/logout → refetch me). `app.tsx`: layout (Nav + `<Outlet/>`).

- [ ] **Step 4: Verify + commit** — `cd web && bun run typecheck && bun run build` (Vite build succeeds). Commit:
```bash
git add web/src
git commit -m "feat(web): client config + react-query hooks + shell + auth guard"
```

---

## Task 3: Login page (magic-link + OAuth)

**Files:** Create `web/src/pages/Login.tsx`

- [ ] **Step 1:** Login page: an email input + "Email me a link" (POST /auth/magic-link/request via the generated SDK; on success show "check your email"); and four buttons "Continue with Google/Microsoft/Apple/GitHub" that navigate to `/auth/{provider}/start` (full-page `window.location.href` — these are server redirects, not SDK calls). Warm-dark/brass styling. After magic-link consume / OAuth callback, the server sets the cookie + redirects to `/`; the guard then sees `useMe()` and shows the dashboard.
- [ ] **Step 2:** `cd web && bun run build` succeeds. Commit:
```bash
git add web/src/pages/Login.tsx web/src/routes.tsx
git commit -m "feat(web): login page (magic-link + OAuth)"
```

---

## Task 4: Billing page

**Files:** Create `web/src/pages/Billing.tsx`

- [ ] **Step 1:** Show current subscription (`useSubscription()` → plan, status, active, period end, founding badge). Three plan cards (Founding $10 / Standard $15 / Pro $25) with "Subscribe" → `useCheckout(plan)` (POST /billing/checkout) → `window.location.href = url`. If active, show "Manage" → POST /billing/portal → redirect. Founding rate messaging.
- [ ] **Step 2:** `bun run build` succeeds. Commit:
```bash
git add web/src/pages/Billing.tsx
git commit -m "feat(web): billing page (plans, checkout, manage)"
```

---

## Task 5: Registry pages (Handles, Machines, Keys) with mint-once modals

**Files:** Create `web/src/pages/{Handles,Machines,Keys}.tsx`, `web/src/components/SecretModal.tsx`

- [ ] **Step 1: SecretModal** — a modal that shows a secret ONCE with a copy button + "I saved it" to dismiss; warns it won't be shown again.
- [ ] **Step 2: Handles** — list handles (`useHandles`), a "Claim handle" form with live availability check (GET /handles/availability?name=) and validation hints, claim (POST /handles); revoke (DELETE).
- [ ] **Step 3: Machines (agents)** — list agents (prefix only), "Pair a machine" (pick a handle + name → POST /agents → show the returned `ak_agent_` token in SecretModal with setup instructions: paste into the agent's `ALTKEY_AGENT_TOKEN`); unpair (DELETE).
- [ ] **Step 4: Keys** — list keys (prefix only), "Create key" (name + optional machine → POST /keys → show `ak_live_` secret in SecretModal with "use as OPENAI_API_KEY"); revoke (DELETE).
- [ ] **Step 5:** `bun run build` succeeds. Commit:
```bash
git add web/src/pages/Handles.tsx web/src/pages/Machines.tsx web/src/pages/Keys.tsx web/src/components/SecretModal.tsx
git commit -m "feat(web): registry pages (handles/machines/keys) + mint-once secret modal"
```

---

## Task 6: Usage + Adapters + Dashboard home

**Files:** Create `web/src/pages/{Usage,Adapters,Dashboard}.tsx`

- [ ] **Step 1: Usage** — `useUsageSummary()` → a simple table/bars of rollups (period, model, requests, tokens, bytes). Totals header. (A lightweight inline SVG bar chart is fine; no chart lib needed.)
- [ ] **Step 2: Adapters** — `useAdapters()` → catalog cards (name, description, version, target tool).
- [ ] **Step 3: Dashboard home** — a status overview: subscription state, handle + tunnel URL (`https://<handle>.altkey.app/v1`), # machines, # keys, a "quick start" checklist (subscribe → claim handle → pair machine → mint key). Links to each page.
- [ ] **Step 4:** `bun run build` succeeds. Commit:
```bash
git add web/src/pages/Usage.tsx web/src/pages/Adapters.tsx web/src/pages/Dashboard.tsx web/src/routes.tsx
git commit -m "feat(web): usage, adapters, dashboard home"
```

---

## Task 7: Warm-dark/brass design system + final build

**Files:** Create `web/src/styles/theme.css`; modify pages/components for styling

- [ ] **Step 1: Theme** — `web/src/styles/theme.css`: CSS custom properties for the warm-dark/brass palette (deep warm-charcoal backgrounds, brass/amber accents, off-white text), typography, spacing, card/button/input styles, modal styles. Import in `main.tsx`. Apply classes across the pages for a cohesive dev-tool aesthetic (Linear/Vercel-like).
- [ ] **Step 2: Final build + typecheck** — `cd web && bun run gen && bun run typecheck && bun run build` all succeed; `dist/` produced.
- [ ] **Step 3: Confirm contract-first guard works** — temporarily reference a non-existent SDK function in a scratch line, run `bun run typecheck`, confirm it FAILS (proving the contract-first rule), then remove the scratch line. Document this in the commit message.
- [ ] **Step 4: Commit**
```bash
git add web/src/styles web/src
git commit -m "feat(web): warm-dark/brass design system + final build (contract-first verified)"
```

---

## Self-Review

**Spec coverage (3f slice):** Implements the spec's "React dashboard" + the contract-first rule: a Vite/React/TS app whose client is generated by `@hey-api/openapi-ts` from the control-plane's dumped OpenAPI (Task 1), consuming only generated SDK functions (so a missing endpoint/model fails `tsc` — Task 7 Step 3 proves it). Pages cover the full dashboard surface from the spec: login (magic-link + 4 OAuth), billing (checkout/portal), registry (claim handle / pair machine / mint key — mint-once secret modals), usage analytics, adapters catalog, and a quick-start home. Warm-dark/brass design. Out of scope: the desktop app (Plan 4), real provider credentials (deploy-time).

**Placeholder scan:** The two reconciliation points (the @hey-api generated client's exact config API in Task 2 Step 1, and the `dump_openapi` app.rs refactor in Task 1) specify the required OUTCOME and how to adapt — real adaptation to the installed toolchain/codegen, not gaps. The `router_parts` refactor is explicit (extract the route chain; `build` must behave identically — guarded by `cargo test -p control-plane` still passing).

**Type/contract consistency:** the generated `src/client` is the single typed contract; `queries.ts` hooks wrap only generated SDK fns; pages call only those hooks. The dumped `openapi.json` == the served spec because both come from the same `router_parts(state)` (Task 1 refactor). Verification is build-based (codegen + tsc + vite build) at every task — appropriate for a frontend (no Rust-style unit tests; the typecheck IS the contract test).

**Security:** the SPA holds no secrets; auth is the httpOnly session cookie (`credentials: 'include'`); mint-once `ak_agent_`/`ak_live_` secrets are shown once in a modal and never persisted client-side; OAuth/magic-link are server redirects (the SPA never handles provider tokens). All prior tracked follow-ups (verified-email gate, prod relay CONTROL_PLANE_URL, webhook freshness) are backend concerns, unaffected here.
