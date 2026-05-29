# altkey — All-OAuth Engine + Rust Port — Design Spec

**Date:** 2026-05-29
**Branch:** `dev`
**Status:** Approved (design), pre-implementation
**Sub-project:** 1 of N (engine first; multi-tenant/auth/billing are later specs)

---

## 0. Goal

Two converging changes:

1. **Go all-OAuth.** Move ChatGPT and Gemini to direct OAuth/CLI-token connect
   (like Claude already is). **Drop the cookie relay, the browser extension, the
   ChatGPT proof-of-work, the Chrome-TLS impersonation, and the whole
   Cloudflare-fighting machinery.** Every provider connects directly to its real
   API with a refreshable OAuth token.
2. **Rewrite the backend in Rust** for the hundreds-to-thousands concurrent
   target — same product, same behavior, just an efficient runtime.

The Python `local/` build is the **reference oracle** — it stays working and
validates the Rust port.

## 0.1 Why all-OAuth is strictly better here
| Cookie relay (being removed) | All-OAuth (target) |
|---|---|
| Chrome **TLS impersonation** (the `rquest` gamble) | Plain HTTPS — no impersonation |
| ChatGPT **proof-of-work** solver | Gone (Codex API doesn't need it) |
| Fights **Cloudflare** | No fight |
| Cookies **expire weekly**, browser-refresh | Refreshable OAuth tokens |
| Needs the **extension** | One-click connect, no extension |

This also **eliminates the single biggest risk** the prior draft was anchored
on (whether a Rust client could impersonate Chrome well enough). It can't fail
because we no longer need it.

## 0.2 Scope
**In scope:** the all-OAuth backend, **single-tenant** (operator's own tokens),
reproducing every *non-cookie* capability of the Python backend.

**Out of scope (later sub-projects):** multi-tenant accounts, per-user vault,
billing, abuse controls.

**Dropped entirely (not ported):** cookie relay, `/admin/capture`,
`/admin/import`, the browser extension (`local/extension/`), the ChatGPT PoW,
TLS impersonation, the legacy `claude.ai` chat relay.

**Reused unchanged (not Python):** `dashboard.html`, the transparent-mode
certs/PowerShell scripts (`local/transparent/` — orthogonal to cookies; it just
runs the server on 443 with a cert and accepts any key).

---

## 1. Sequencing — two phases

The new OAuth providers (ChatGPT Codex, Gemini CLI) are **not built or validated
yet** — only Claude OAuth is proven. Reverse-engineering is far faster to nail
in Python (our proven pattern). So:

**Phase 0 — all-OAuth in Python (precursor):**
- Build **ChatGPT via Codex OAuth** (`~/.codex/auth.json` → OpenAI Responses API).
- Build **Gemini via CLI OAuth** (Google login → Gemini API).
- Remove cookie-relay providers + endpoints + extension.
- Validate live (completion, stream, tool call, vision) for all three.
- Result: a clean all-OAuth Python backend = the **reference** for Phase 1.

**Phase 1 — Rust port:**
- Port the proven all-OAuth Python engine to Rust (axum + reqwest + tokio).
- Validate byte-for-byte against the Phase-0 Python reference.

This spec covers the **target design** for both phases. The implementation plan
will run Phase 0, then Phase 1.

---

## 2. Feature-parity checklist (target = the all-OAuth backend)

Endpoints:
- `POST /v1/chat/completions` — OpenAI-compatible, streaming + non-streaming, tool calls.
- `POST /v1/messages` — native Anthropic.
- `GET  /v1/models` — catalog, scope-filtered.
- `GET  /` — dashboard.
- `GET  /callback`, `POST /admin/oauth/start|finish` — Connect-with-Claude (+ ChatGPT/Gemini equivalents).
- `POST /admin/connect-cli` — read local CLI creds (Claude Code / Codex / Gemini CLI).
- `GET  /admin/status`, `POST /admin/detect`, `POST /admin/disconnect`.
- `POST /admin/keys`, `GET /admin/keys`, `POST /admin/keys/revoke` — keys + provider scope.

Providers (all via OAuth/direct API; all: text + streaming + vision):
- **Claude** → OAuth → `api.anthropic.com` (proven). Tool calling, real usage.
- **ChatGPT** → Codex OAuth → OpenAI Responses API. Tool calling.
- **Gemini** → CLI OAuth → Google Gemini API. Tool calling.

Capabilities:
- OpenAI↔Anthropic↔(OpenAI Responses)↔(Gemini) translation, both directions.
- SSE streaming with tool-call deltas.
- Model detection (via each real API's models endpoint) + alias resolution + catalogs.
- Provider-scoped API keys + enforcement.
- Dual-protocol auth (Bearer + `x-api-key`).
- OAuth token refresh + server seeding (env secret) + proactive refresh loop.
- Admin-token gating on `/admin/*`.
- Transparent mode (server on 443 + cert, accept any key).
- Scrubbing logger.

---

## 3. Architecture (target)

```
client ──► axum server (tokio, single async runtime)

  POST /v1/chat/completions → auth(sk-alt) → scope → route(model)
        → provider.stream → translate(provider-native → OpenAI) → SSE out
  POST /v1/messages         → auth → claude passthrough → raw Anthropic SSE
  GET  /v1/models           → catalog (scope-filtered)
  /admin/*, /oauth/*, /callback, /                                  (control plane)

providers (ALL plain HTTPS with an OAuth bearer — NO impersonation):
  claude  → reqwest → api.anthropic.com           (OAuth + attestation)
  chatgpt → reqwest → chatgpt.com/backend-api      (Codex OAuth, Responses API)
  gemini  → reqwest → generativelanguage / Gemini  (CLI OAuth)

store: SQLite (sqlx) — OAuth tokens (per provider), sk-alt keys (+scope), model cache
```

Pure I/O; tokio holds many streams cheaply. **No CPU-heavy PoW anymore.** One
ordinary HTTP client (`reqwest`) for everything — the only auth difference is
which OAuth bearer + headers each provider needs.

---

## 4. Module layout (Rust, Phase 1)

```
altkey-rs/  (new crate, on dev branch alongside local/)
  Cargo.toml      axum, tokio, reqwest, serde, serde_json, sqlx(sqlite),
                  sha2, base64, rand, tracing
  src/
    main.rs       axum app, routes, runtime, startup (seed + refresh loop)
    config.rs     env (HOST/PORT, ADMIN_TOKEN, TRANSPARENT, TLS_CERT/KEY, at-rest key)
    auth.rs       sk-alt / x-api-key; admin gate; scope resolve
    store.rs      sqlite: tokens, keys(+scope), model cache; at-rest encryption (chacha20poly1305)
    oauth.rs      PKCE connect + refresh + seed (Claude verified; ChatGPT/Gemini per Phase 0)
    translate.rs  OpenAI ↔ Anthropic / Responses / Gemini
    sse.rs        OpenAI chunk writer + Anthropic passthrough
    models.rs     catalog, alias resolver, per-account detection
    log.rs        scrubbing logger
    providers/
      mod.rs            routing by model prefix
      claude.rs         Anthropic API
      chatgpt.rs        Codex OAuth → Responses API
      gemini.rs         CLI OAuth → Gemini API
  tests/
    translate_test.rs   both-direction translation units
    parity/             fixtures captured from the Phase-0 Python reference
```

No `pow.rs`, no impersonation layer, no cookie/extension modules.

## 4.1 Known/required OAuth params
- **Claude (verified):** client_id `9d1c250a-…`, authorize `claude.ai/oauth/authorize`,
  redirect `http://localhost:<port>/callback`, scope `user:inference`, PKCE S256,
  32-byte state, token `console.anthropic.com/v1/oauth/token`, beta
  `oauth-2025-04-20`, attestation "You are Claude Code, Anthropic's official CLI for Claude."
- **ChatGPT (Phase 0 to confirm):** Codex `access_token` + `account_id` from
  `~/.codex/auth.json`; OpenAI Responses API (`/backend-api/codex/responses` or
  current); refresh flow; tool calling native.
- **Gemini (Phase 0 to confirm):** Gemini CLI OAuth creds; Google Gemini API
  (`generativelanguage.googleapis.com`); tool calling native.

---

## 5. Validation — parity against Python

Phase 0 produces the clean all-OAuth Python backend. Phase 1 (Rust) is validated
against it: capture fixtures (exact upstream requests + client output) per
capability; Rust must match byte-for-byte. Live smoke tests per provider
(completion, stream, tool call, vision). No live calls in CI — fixtures only.

---

## 6. Error handling
Mirror Python's taxonomy/codes: 401 (key), 403 (killed/scope), 400 (unknown
model / not connected), 502 (`upstream error`), 429 (quota), session-expired
path. Streaming errors → final SSE error chunk + `[DONE]`.

---

## 7. Risks

| Risk | Mitigation |
|---|---|
| ChatGPT Codex OAuth path differs / needs Arkose | Phase 0 spike in Python first; we already have the token |
| Gemini CLI OAuth quirks | Phase 0 spike in Python first |
| Re-introducing fixed bugs in the Rust port | Parity fixtures from Phase-0 Python |
| Scope creep into multi-tenant | Hard scope line (§0.2) |

(The former #1 risk — Rust TLS impersonation — is **gone**; all-OAuth needs no impersonation.)

---

## 8. Out of scope
- Multi-tenant accounts, per-user vault, billing — later sub-projects.
- Deployment/hosting — separate concern (Rust binary is deploy-friendly).
- `hosted/` Workers scaffold — abandoned (Workers can't host this anyway).

---

## 9. Definition of done
- **Phase 0:** all three providers via OAuth in Python (completion, stream, tool
  call, vision); cookie relay + extension removed.
- **Phase 1:** Rust engine reproduces every §2 endpoint/provider/capability;
  parity fixtures pass; live smoke tests match Python; dashboard + transparent
  scripts work unchanged against the Rust server.
