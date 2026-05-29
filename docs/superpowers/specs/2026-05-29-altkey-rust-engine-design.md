# altkey — Rust Backend Engine (port) — Design Spec

**Date:** 2026-05-29
**Branch:** `dev`
**Status:** Approved (design), pre-implementation
**Sub-project:** 1 of N (engine first; multi-tenant/auth/billing are later specs)

---

## 0. Goal

Reimplement altkey's **backend** in Rust with **full feature parity** to the
current Python (`local/`) version. Same product, same behavior, same endpoints,
same providers — just a more efficient, scalable runtime for the
hundreds-to-thousands concurrent-stream target.

**This is a performance/scale rewrite, not a product change. No feature the
Python backend has today may be dropped.**

The Python `local/` implementation is the **reference oracle** — it stays
working and is used to validate the Rust port byte-for-byte.

## 0.1 Scope

**In scope (this spec):** the Rust backend engine, **single-tenant** (one set of
the operator's own credentials), reproducing every Python backend capability.

**Out of scope (later sub-projects):** multi-tenant accounts, per-user token
vault, billing, abuse controls. The engine is built so these layer on top later.

## 0.2 Reused unchanged (NOT ported — they're not Python)
These already work and just point at the Rust server:
- `local/extension/` — MV3 browser extension (JavaScript).
- `local/app/dashboard.html` — dashboard UI (static HTML/JS).
- `local/transparent/` — CA/cert generator + PowerShell setup/teardown scripts.

---

## 1. Feature-parity checklist (MUST all exist in Rust)

Endpoints:
- `POST /v1/chat/completions` — OpenAI-compatible, streaming + non-streaming, tool calls.
- `POST /v1/messages` — native Anthropic (for Anthropic-SDK clients).
- `GET  /v1/models` — catalog, filtered by the key's provider scope.
- `GET  /` — serve dashboard HTML.
- `GET  /callback`, `POST /admin/oauth/start`, `POST /admin/oauth/finish` — Connect-with-Claude OAuth.
- `POST /admin/capture`, `POST /admin/import` — extension/manual cookie ingest.
- `POST /admin/connect-cli` — read local Claude Code credentials.
- `GET  /admin/status`, `POST /admin/detect`, `POST /admin/connect`, `POST /admin/disconnect`.
- `POST /admin/keys`, `GET /admin/keys`, `POST /admin/keys/revoke` — key mgmt (incl. provider scope).

Providers (all: text + streaming + vision):
- **Claude via OAuth** → real Anthropic API (tool calling, real usage). *Primary.*
- **Claude chat-relay** → claude.ai (legacy fallback).
- **ChatGPT** → chatgpt.com (accessToken + **sentinel proof-of-work** + GPT-5 models).
- **Gemini** → gemini.google.com (SNlM0e + StreamGenerate).

Capabilities:
- OpenAI↔Anthropic translation (messages, tools, vision image blocks).
- SSE streaming with tool-call deltas.
- Model **auto-detection** per account + **alias resolution** + catalogs.
- **Provider-scoped API keys** (single-provider or all-providers) + enforcement.
- **Dual-protocol auth** (Bearer + `x-api-key`).
- **Transparent mode** (`ALTKEY_TRANSPARENT`, TLS on 443, accept any key).
- OAuth token **refresh** + server-side **seeding** (`ALTKEY_CLAUDE_OAUTH_JSON`) + proactive refresh loop.
- Admin-token gating (`ALTKEY_ADMIN_TOKEN`) on `/admin/*`.
- Scrubbing logger (never log cookies/tokens/prompts).

---

## 2. Architecture

```
client ──► axum server (tokio, single async runtime)

  POST /v1/chat/completions → auth(sk-alt) → scope check → route(model)
        → provider.stream → translate(Anthropic/web → OpenAI) → SSE out
  POST /v1/messages         → auth → claude_oauth passthrough → raw Anthropic SSE out
  GET  /v1/models           → catalog (scope-filtered)
  /admin/*, /oauth/*, /callback, /                                  (control plane)

providers (one rquest client, two TLS modes):
  claude_oauth → rquest plain TLS → api.anthropic.com   (OAuth bearer + attestation)  PRIMARY
  chatgpt      → rquest CHROME-impersonate → chatgpt.com  (+ SHA3-512 PoW)
  gemini       → rquest CHROME-impersonate → gemini.google.com
  claude_chat  → rquest CHROME-impersonate → claude.ai    (legacy)

store: SQLite (sqlx) — OAuth token, sk-alt keys (+ scope), detected models
```

**Two TLS modes from one client:** the OAuth/Anthropic call is ordinary HTTPS
(no impersonation — it's the real API). The chat-relay calls need `rquest`'s
Chrome TLS/JA3 + HTTP2 fingerprint (the Rust equivalent of `curl_cffi`).

**Concurrency:** proxying is I/O-bound; tokio holds many streams cheaply. The
only CPU work is the ChatGPT PoW (SHA3-512 loop) — run on `spawn_blocking` so it
never stalls the async reactor.

---

## 3. Module layout

```
altkey-rs/                 (new crate, alongside local/ on the dev branch)
  Cargo.toml               axum, tokio, rquest, serde, serde_json, sqlx(sqlite),
                           sha3, sha2, base64, rand, tracing
  src/
    main.rs                axum app, routes, tokio runtime, startup (seed + refresh loop)
    config.rs              env: ALTKEY_HOST/PORT, ADMIN_TOKEN, TRANSPARENT, TLS_CERT/KEY, FERNET-equiv
    auth.rs                ← _auth(): sk-alt / x-api-key; admin-token gate; scope resolve
    store.rs               ← store.py: sqlite (encrypted-at-rest token, keys, model cache)
    oauth.rs               ← claude_oauth: PKCE connect (verified params) + refresh + seed
    translate.rs           ← OpenAI↔Anthropic (messages, tools, vision)
    sse.rs                 ← OpenAI chunk writer + Anthropic passthrough
    pow.rs                 ← ChatGPT sentinel SHA3-512 solver (spawn_blocking)
    models.rs              ← catalog, alias resolver, per-account detection
    log.rs                 ← scrubbing logger (strip cookies/sk-alt/tokens)
    providers/
      mod.rs               ← routing by model prefix (oauth-preferred for claude)
      claude_oauth.rs      ← real Anthropic API: /v1/messages, tools, stream, vision, model detect
      chatgpt.rs           ← accessToken + chat-requirements + PoW + parse + upload(vision)
      gemini.rs            ← SNlM0e scrape + StreamGenerate + content-push upload(vision)
      claude_chat.rs       ← claude.ai relay (legacy)
  tests/
    translate_test.rs      unit: OpenAI↔Anthropic both directions
    pow_test.rs            unit: PoW correctness
    parity/                fixtures captured from the Python reference
```

Each module maps to a Python file we already debugged → port with the working
file open beside it.

## 3.1 Known constants to carry over verbatim (already solved)
- Claude OAuth: client_id `9d1c250a-e61b-44d9-88ed-5944d1962f5e`, authorize
  `https://claude.ai/oauth/authorize`, redirect `http://localhost:<port>/callback`,
  scope `user:inference`, PKCE S256, **32-byte state**, token endpoint
  `https://console.anthropic.com/v1/oauth/token`, beta `oauth-2025-04-20`,
  attestation system prefix "You are Claude Code, Anthropic's official CLI for Claude."
- ChatGPT: `/api/auth/session` → accessToken; `/backend-api/sentinel/chat-requirements`
  → token + PoW (SHA3-512, config array, `gAAAAAB` prefix); `Openai-Sentinel-*` headers;
  GPT-5 slugs from `/backend-api/models`; vision = register→Azure PUT (retry across
  transports)→finalize→poll, then `multimodal_text` + `image_asset_pointer`.
- Gemini: `SNlM0e` from `/app` HTML; `StreamGenerate` with `f.req=[None, json([[prompt],None,None])]`;
  vision via `content-push.googleapis.com` (Push-ID header) → file id in `[[prompt,0,None,[[[id],name]]],None,None]`.
- Model maps/aliases per provider (Opus 4.8 / Sonnet 4.6 / Haiku 4.5; GPT-5 family; Gemini 2.5).

---

## 4. The day-one risk gate: the `rquest` spike

**Before porting anything**, prove the make-or-break dependency: one request to
`https://claude.ai/api/organizations` (or chatgpt.com) through `rquest` with
Chrome impersonation, using a real cookie. Acceptance: status < 400 (not a
Cloudflare 403). 

- **Pass** → `rquest` replicates `curl_cffi`; proceed with the full port.
- **Fail** → STOP. Reassess the client (alternative impersonation crate, or an
  FFI to libcurl-impersonate) before investing in the port. The whole rewrite
  hinges on this.

The OAuth/Anthropic path does not need impersonation, so it's unaffected — but
ChatGPT/Gemini/Claude-chat-relay all depend on the spike passing.

---

## 5. Validation — parity against the Python reference

The Python `local/` build is the oracle. For each capability:
1. Run it through Python, capture (a) the exact upstream request bytes/headers
   Python builds and (b) the OpenAI/Anthropic output it returns → fixtures in
   `tests/parity/`.
2. Rust must build **byte-identical upstream requests** and produce
   **equivalent client output** for the same input.
3. Live smoke tests against the operator's own accounts (Claude OAuth, ChatGPT,
   Gemini): completion, streaming, tool call, vision — must match Python.

Unit tests: translation (both directions), PoW correctness, model resolution,
auth/scope enforcement.

No live provider calls in CI — fixtures only; live tests are manual pre-merge.

---

## 6. Error handling

Mirror the Python error taxonomy and HTTP codes: 401 (bad/missing key), 403
(killed/scope), 400 (unknown model / not connected), 502 (`upstream error` with
message), 429 (quota), plus the `session expired` path. Streaming errors are
emitted as a final SSE `data:` error chunk then `[DONE]`, identical to Python.

---

## 7. Risks

| Risk | Mitigation |
|---|---|
| `rquest` can't impersonate well enough | Day-one spike gates everything (§4) |
| Re-introducing already-fixed bugs | Parity fixtures from Python (§5) |
| PoW stalls the async reactor | `spawn_blocking` |
| Provider request-shape drift during port | Port with the working Python file open; live smoke tests |
| Scope creep into multi-tenant | Hard scope line (§0.1) — engine only |

---

## 8. Out of scope (explicit)

- Multi-tenant accounts, per-user token vault, OAuth-per-user — later sub-project.
- Billing, abuse controls, rate limiting beyond what Python has.
- Deployment/hosting (separate concern; Rust binary is deploy-friendly).
- The `hosted/` Cloudflare Workers scaffold is abandoned (Workers can't do TLS
  impersonation; the Rust server replaces it as the hosted platform later).

---

## 9. Definition of done

- Every endpoint + provider + capability in §1 works in Rust.
- All Python parity fixtures pass.
- Live smoke tests (Claude OAuth/ChatGPT/Gemini: completion, stream, tool call,
  vision) match the Python reference.
- The extension, dashboard, and transparent scripts work unchanged against the
  Rust server.
