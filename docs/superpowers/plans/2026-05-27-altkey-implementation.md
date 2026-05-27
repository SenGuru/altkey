# altkey Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship two parallel deliverables in `C:\Users\gsent\Desktop\altkey\`: (1) `local/` — runnable today self-host proxy with Claude + ChatGPT + Gemini; (2) `hosted/` — complete multi-tenant SaaS code (Workers + Extension + Dashboard) that deploys with secret config only.

**Architecture:** Local = Python + FastAPI + Playwright + SQLite/Fernet, one process, browser-cookie harvest. Hosted = Cloudflare Workers + D1 + MV3 extension client-side encryption + Astro static dashboard, two-tier key model (K_user / K_session / K_worker) per spec §3.4.

**Tech Stack:** Python 3.11+ (local). TypeScript + Hono + Cloudflare Workers + D1 + R2 (hosted backend). Vanilla JS + MV3 (extension). Astro + TypeScript (dashboard). `@noble/ciphers`, `@noble/hashes`, `argon2-browser` (crypto). Polar SDK + NOWPayments REST.

---

## File Structure

```
altkey/
├── docs/superpowers/{specs,plans}/...
├── local/                                ← Track A: runnable today
│   ├── README.md
│   ├── pyproject.toml
│   ├── app/
│   │   ├── __init__.py
│   │   ├── main.py                       FastAPI + admin + /v1/*
│   │   ├── store.py                      SQLite + Fernet cookie vault
│   │   ├── harvester.py                  Playwright headed login flow
│   │   ├── dashboard.html                Single-page admin UI
│   │   └── providers/
│   │       ├── __init__.py               model-prefix → provider routing
│   │       ├── claude.py                 claude.ai SSE → OpenAI chunks
│   │       ├── chatgpt.py                chat.openai.com SSE → OpenAI chunks
│   │       └── gemini.py                 gemini.google.com → OpenAI chunks
│   └── tests/
│       ├── test_store.py
│       ├── test_providers_translate.py
│       └── test_harvester_smoke.py
│
├── hosted/                               ← Track B: code-complete SaaS
│   ├── README.md                         deploy + secrets guide
│   ├── package.json                      workspace root
│   ├── workers/
│   │   ├── wrangler.toml
│   │   ├── package.json
│   │   ├── tsconfig.json
│   │   ├── src/
│   │   │   ├── index.ts                  route splitter (/v1, /api, /sync, /wh)
│   │   │   ├── env.d.ts                  Cloudflare Env binding types
│   │   │   ├── crypto.ts                 Argon2id verify, XChaCha20, HKDF, HMAC, K_worker
│   │   │   ├── db.ts                     D1 queries (prepared statements)
│   │   │   ├── log.ts                    scrubbing logger
│   │   │   ├── auth.ts                   API key middleware, session cookie
│   │   │   ├── quota.ts                  rate limits + abuse caps
│   │   │   ├── routes/
│   │   │   │   ├── proxy.ts              /v1/chat/completions, /v1/models
│   │   │   │   ├── control.ts            /api/signup, /api/unlock, /api/keys/*
│   │   │   │   ├── sync.ts               /sync/upload, /sync/status
│   │   │   │   ├── webhook.ts            /wh/polar, /wh/nowpayments
│   │   │   │   └── canary.ts             /canary.txt
│   │   │   ├── providers/
│   │   │   │   ├── index.ts              model-prefix routing
│   │   │   │   ├── claude.ts             cookie → claude.ai SSE → OpenAI chunks
│   │   │   │   ├── chatgpt.ts            cookie → chat.openai.com SSE → OpenAI chunks
│   │   │   │   └── gemini.ts             cookie → gemini.google.com → OpenAI chunks
│   │   │   ├── billing/
│   │   │   │   ├── polar.ts              checkout creation, webhook verify
│   │   │   │   └── nowpayments.ts        crypto checkout + webhook verify
│   │   │   └── utils/
│   │   │       ├── sse.ts                SSE writer/reader helpers
│   │   │       └── openai.ts             OpenAI chunk builder
│   │   └── migrations/
│   │       └── 0001_init.sql             users, sessions, api_keys, usage, payments
│   ├── extension/
│   │   ├── manifest.json                 MV3, cookies + storage + alarms
│   │   ├── popup.html
│   │   ├── popup.js                      passphrase → K_user, status display
│   │   ├── popup.css
│   │   ├── background.js                 cookies.onChanged → encrypt → upload
│   │   ├── lib/
│   │   │   ├── crypto.js                 Argon2id (WASM), XChaCha20, HKDF, HMAC
│   │   │   ├── sync.js                   POST /sync/upload, retries
│   │   │   └── state.js                  K_user lifecycle, idle lock
│   │   └── icons/                        16/48/128 placeholders
│   ├── dashboard/
│   │   ├── package.json
│   │   ├── astro.config.mjs
│   │   ├── tsconfig.json
│   │   ├── src/
│   │   │   ├── pages/
│   │   │   │   ├── index.astro           landing + threat model
│   │   │   │   ├── app.astro             authed dashboard
│   │   │   │   ├── billing.astro         checkout entry
│   │   │   │   ├── docs.astro            integration recipes
│   │   │   │   └── canary.txt.ts         server-rendered canary
│   │   │   ├── components/
│   │   │   │   ├── StatusDots.astro
│   │   │   │   ├── KeyList.astro
│   │   │   │   └── BillingPanel.astro
│   │   │   ├── lib/
│   │   │   │   ├── crypto.ts             same primitives as extension
│   │   │   │   └── api.ts                fetch wrappers
│   │   │   └── styles/global.css
│   │   └── public/favicon.svg
│   └── ops/
│       ├── canary.sh                     weekly cron: BTC block hash + sign
│       └── deploy.md                     wrangler deploy + secret commands
│
└── README.md                             top-level: links to local/ and hosted/
```

---

## Track A — Local (runnable today)

### Phase A1: Restructure existing scaffold into `local/`

- [ ] Move existing `app/`, `pyproject.toml`, `README.md` into `local/`.
- [ ] Update import paths if any are absolute (none expected).
- [ ] Add `local/tests/` directory.
- [ ] Commit: `chore: move scaffold into local/ track`.

### Phase A2: ChatGPT provider (real implementation)

**File:** `local/app/providers/chatgpt.py`

ChatGPT's web backend differs from Claude. The flow:
1. Get `accessToken` from `https://chatgpt.com/api/auth/session` using the `__Secure-next-auth.session-token` cookie.
2. POST to `https://chatgpt.com/backend-api/conversation` with that bearer token.
3. Stream returns SSE with `data: {...}` lines where each has `message.content.parts` for the assistant.

**Real implementation (writeable end-to-end):**

```python
import json
import time
import uuid
from typing import AsyncIterator
import httpx
from .. import store
from ..harvester import cookie_header

NAME = "chatgpt"
MODELS = ["gpt-4o", "gpt-4o-mini", "gpt-4-1", "o1", "o3", "o4-mini", "chatgpt-4o-latest"]

_BASE = "https://chatgpt.com"
_DOMAINS = ("chatgpt.com", "chat.openai.com", "openai.com")


def _headers(session: dict, access_token: str | None = None) -> dict:
    h = {
        "User-Agent": session.get("user_agent") or "Mozilla/5.0",
        "Accept": "text/event-stream",
        "Accept-Language": "en-US,en;q=0.9",
        "Origin": _BASE,
        "Referer": f"{_BASE}/",
        "Cookie": cookie_header(session, _DOMAINS),
    }
    if access_token:
        h["Authorization"] = f"Bearer {access_token}"
        h["Content-Type"] = "application/json"
    return h


async def _access_token(client: httpx.AsyncClient, session: dict) -> str:
    r = await client.get(f"{_BASE}/api/auth/session", headers=_headers(session))
    r.raise_for_status()
    data = r.json()
    token = data.get("accessToken")
    if not token:
        raise RuntimeError("chatgpt session expired — re-connect in dashboard")
    return token


def _to_parts(messages: list[dict]) -> list[dict]:
    out = []
    for m in messages:
        role = m.get("role", "user")
        content = m.get("content")
        if isinstance(content, list):
            content = "".join(p.get("text", "") for p in content if isinstance(p, dict))
        if not isinstance(content, str):
            content = str(content)
        out.append({
            "id": str(uuid.uuid4()),
            "author": {"role": "system" if role == "system" else role},
            "content": {"content_type": "text", "parts": [content]},
            "metadata": {},
        })
    return out


async def stream(req: dict) -> AsyncIterator[dict]:
    session = store.load_session("chatgpt")
    if not session:
        raise RuntimeError("chatgpt not connected")

    model = req.get("model", "gpt-4o")
    parts = _to_parts(req.get("messages", []))

    async with httpx.AsyncClient(http2=True, timeout=httpx.Timeout(120.0, read=300.0)) as client:
        token = await _access_token(client, session)
        payload = {
            "action": "next",
            "messages": parts,
            "parent_message_id": str(uuid.uuid4()),
            "model": model,
            "timezone_offset_min": 420,
            "history_and_training_disabled": False,
            "conversation_mode": {"kind": "primary_assistant"},
            "force_paragen": False,
            "force_rate_limit": False,
        }
        url = f"{_BASE}/backend-api/conversation"
        last_text = ""
        async with client.stream("POST", url, headers=_headers(session, token), json=payload) as resp:
            if resp.status_code >= 400:
                body = await resp.aread()
                raise RuntimeError(f"chatgpt error {resp.status_code}: {body[:400]!r}")
            async for line in resp.aiter_lines():
                if not line or not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                if not data or data == "[DONE]":
                    continue
                try:
                    evt = json.loads(data)
                except json.JSONDecodeError:
                    continue
                msg = evt.get("message") or {}
                content = msg.get("content") or {}
                parts_list = content.get("parts") or []
                if not parts_list:
                    continue
                text = parts_list[0] if isinstance(parts_list[0], str) else ""
                if text and text.startswith(last_text):
                    delta = text[len(last_text):]
                    last_text = text
                    if delta:
                        yield {"delta": delta}
                elif text:
                    last_text = text
                    yield {"delta": text}
```

- [ ] Write the file as above.
- [ ] Add cookie-header domain expansion for chatgpt.com to `harvester.py:_DOMAINS_BY_PROVIDER`.
- [ ] Update `harvester.py:_LOGIN_URLS` chatgpt entry to `https://chatgpt.com/`.
- [ ] Update `harvester.py:_COOKIE_KEYS` chatgpt to include `__Secure-next-auth.session-token`.
- [ ] Commit: `feat(local): implement ChatGPT provider`.

### Phase A3: Gemini provider (real implementation)

**File:** `local/app/providers/gemini.py`

Gemini's backend uses `StreamGenerate` with batched RPC requests. Needs `SNlM0e` token scraped from `gemini.google.com/app` HTML on each call.

```python
import json
import re
import time
import uuid
from typing import AsyncIterator
import httpx
from .. import store
from ..harvester import cookie_header

NAME = "gemini"
MODELS = ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash", "gemini-1.5-pro"]

_BASE = "https://gemini.google.com"
_DOMAINS = ("google.com",)
_SNLM_RE = re.compile(r'"SNlM0e":"([^"]+)"')


def _headers(session: dict) -> dict:
    return {
        "User-Agent": session.get("user_agent") or "Mozilla/5.0",
        "Accept": "*/*",
        "Accept-Language": "en-US,en;q=0.9",
        "Origin": _BASE,
        "Referer": f"{_BASE}/app",
        "Cookie": cookie_header(session, _DOMAINS),
    }


async def _snlm0e(client: httpx.AsyncClient, session: dict) -> str:
    r = await client.get(f"{_BASE}/app", headers=_headers(session))
    r.raise_for_status()
    m = _SNLM_RE.search(r.text)
    if not m:
        raise RuntimeError("gemini session expired — re-connect in dashboard")
    return m.group(1)


def _flatten(messages: list[dict]) -> str:
    parts = []
    for m in messages:
        role = m.get("role", "user")
        content = m.get("content")
        if isinstance(content, list):
            content = "".join(p.get("text", "") for p in content if isinstance(p, dict))
        if not isinstance(content, str):
            content = str(content)
        tag = {"user": "User", "assistant": "Assistant", "system": "System"}.get(role, role.title())
        parts.append(f"{tag}: {content}")
    return "\n\n".join(parts)


_MODEL_HEADER = {
    "gemini-2.5-pro": ["c_a065d44e", 1],
    "gemini-2.5-flash": ["c_a065d44e", 0],
    "gemini-2.0-flash": ["c_835d8b8c", 0],
    "gemini-1.5-pro": ["c_70f59a40", 1],
}


async def stream(req: dict) -> AsyncIterator[dict]:
    session = store.load_session("gemini")
    if not session:
        raise RuntimeError("gemini not connected")

    model = req.get("model", "gemini-2.5-flash")
    prompt = _flatten(req.get("messages", []))

    async with httpx.AsyncClient(http2=True, timeout=httpx.Timeout(180.0, read=300.0)) as client:
        snlm = await _snlm0e(client, session)
        model_hdr = _MODEL_HEADER.get(model, _MODEL_HEADER["gemini-2.5-flash"])
        req_id = str(uuid.uuid4())

        inner = [[prompt], None, [None, None, None, [], None, None, "", 0, 0, 0, [], 0, 0, None, 0, 0, [], 0, 0, model_hdr]]
        f_req = json.dumps([None, json.dumps(inner)])
        form = {"f.req": f_req, "at": snlm}
        url = f"{_BASE}/_/BardChatUi/data/assistant.lamda.BardFrontendService/StreamGenerate"
        params = {"bl": "boq_assistant-bard-web-server", "_reqid": req_id, "rt": "c"}

        last_text = ""
        async with client.stream("POST", url, headers=_headers(session), params=params, data=form) as resp:
            if resp.status_code >= 400:
                body = await resp.aread()
                raise RuntimeError(f"gemini error {resp.status_code}: {body[:400]!r}")
            buf = ""
            async for chunk in resp.aiter_text():
                buf += chunk
                while True:
                    nl = buf.find("\n")
                    if nl < 0:
                        break
                    line = buf[:nl].strip()
                    buf = buf[nl + 1:]
                    if not line or line.startswith(")]}'"):
                        continue
                    try:
                        outer = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    for row in outer:
                        if not isinstance(row, list) or len(row) < 3:
                            continue
                        try:
                            inner_json = row[2]
                            if not isinstance(inner_json, str):
                                continue
                            data = json.loads(inner_json)
                            cands = data[4] if len(data) > 4 else None
                            if not cands:
                                continue
                            text = cands[0][1][0] if cands[0] and len(cands[0]) > 1 and cands[0][1] else ""
                            if text and text.startswith(last_text):
                                delta = text[len(last_text):]
                                last_text = text
                                if delta:
                                    yield {"delta": delta}
                            elif text:
                                last_text = text
                                yield {"delta": text}
                        except (IndexError, json.JSONDecodeError, TypeError):
                            continue
```

- [ ] Write the file as above.
- [ ] Update `harvester.py:_DOMAINS_BY_PROVIDER` gemini to include `google.com`.
- [ ] Update `harvester.py:_COOKIE_KEYS` gemini to ensure all three PSID variants captured.
- [ ] Commit: `feat(local): implement Gemini provider`.

### Phase A4: Tests for `local/`

**File:** `local/tests/test_store.py`

```python
import os
import tempfile
from pathlib import Path
import pytest


@pytest.fixture(autouse=True)
def tmp_store(monkeypatch, tmp_path):
    monkeypatch.setenv("ALTKEY_HOME", str(tmp_path))
    import importlib
    from app import store
    importlib.reload(store)
    yield store


def test_session_roundtrip(tmp_store):
    tmp_store.init()
    tmp_store.save_session("claude", {"cookies": [{"name": "sessionKey", "value": "x"}], "user_agent": "UA"})
    got = tmp_store.load_session("claude")
    assert got["cookies"][0]["value"] == "x"
    assert got["user_agent"] == "UA"


def test_session_missing(tmp_store):
    tmp_store.init()
    assert tmp_store.load_session("claude") is None


def test_api_key_lifecycle(tmp_store):
    tmp_store.init()
    key = tmp_store.mint_key("test")
    assert key.startswith("sk-alt-")
    assert tmp_store.key_exists(key) is True
    tmp_store.revoke_key(key)
    assert tmp_store.key_exists(key) is False


def test_delete_session(tmp_store):
    tmp_store.init()
    tmp_store.save_session("claude", {"cookies": [], "user_agent": "x"})
    tmp_store.delete_session("claude")
    assert tmp_store.load_session("claude") is None
```

**File:** `local/tests/test_providers_translate.py`

```python
import pytest
from app.providers import for_model, list_models
from app.providers import claude, chatgpt, gemini


def test_for_model_routing():
    assert for_model("claude-sonnet-4-5") is claude
    assert for_model("gpt-4o") is chatgpt
    assert for_model("o3") is chatgpt
    assert for_model("gemini-2.5-pro") is gemini
    assert for_model("unknown-model") is None


def test_list_models_contains_each_provider():
    models = list_models()
    ids = [m["id"] for m in models]
    assert any(m.startswith("claude-") for m in ids)
    assert any(m.startswith("gpt-") for m in ids)
    assert any(m.startswith("gemini-") for m in ids)


def test_claude_flatten_basic():
    sys, prompt = claude._flatten([
        {"role": "system", "content": "be brief"},
        {"role": "user", "content": "hi"},
    ])
    assert sys == "be brief"
    assert prompt.endswith("Assistant:")
    assert "Human: hi" in prompt


def test_claude_openai_chunk_shape():
    c = claude.openai_chunk("claude-sonnet-4-5", "hello")
    assert c["object"] == "chat.completion.chunk"
    assert c["choices"][0]["delta"]["content"] == "hello"
    assert c["choices"][0]["finish_reason"] is None


def test_chatgpt_to_parts_handles_list_content():
    parts = chatgpt._to_parts([
        {"role": "user", "content": [{"type": "text", "text": "hi"}, {"type": "text", "text": " there"}]}
    ])
    assert parts[0]["content"]["parts"] == ["hi there"]
```

- [ ] Write both test files.
- [ ] Run `pytest local/tests/ -v` — expect all green.
- [ ] Commit: `test(local): add unit tests for store + provider translators`.

### Phase A5: Polish dashboard + README

- [ ] Update `local/app/dashboard.html`: enable ChatGPT + Gemini Connect buttons (remove `disabled`), update provider list to remove "stub" notes.
- [ ] Update `local/README.md`: full setup, model name table, "what each provider needs" notes about cookie expiry.
- [ ] Commit: `chore(local): polish dashboard + README`.

---

## Track B — Hosted (code-complete scaffold)

### Phase B1: Workspace + workers package layout

**File:** `hosted/package.json`

```json
{
  "name": "altkey-hosted",
  "private": true,
  "workspaces": ["workers", "dashboard"]
}
```

**File:** `hosted/workers/package.json`

```json
{
  "name": "altkey-workers",
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "wrangler dev",
    "deploy": "wrangler deploy",
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "dependencies": {
    "hono": "^4.6.10",
    "@noble/ciphers": "^1.0.0",
    "@noble/hashes": "^1.5.0"
  },
  "devDependencies": {
    "@cloudflare/workers-types": "^4.20240909.0",
    "wrangler": "^3.78.0",
    "vitest": "^2.1.0",
    "typescript": "^5.6.0"
  }
}
```

**File:** `hosted/workers/wrangler.toml`

```toml
name = "altkey"
main = "src/index.ts"
compatibility_date = "2026-05-01"
compatibility_flags = ["nodejs_compat"]

[[d1_databases]]
binding = "DB"
database_name = "altkey"
database_id = "REPLACE_WITH_YOUR_D1_ID"

[vars]
CANARY_PUBLIC_KEY = ""
POLAR_WEBHOOK_SECRET = ""
NOWPAYMENTS_IPN_SECRET = ""

# Secrets (set via wrangler secret put):
# - CF_SECRET                (32-byte hex, used to derive K_worker)
# - POLAR_API_KEY
# - NOWPAYMENTS_API_KEY
```

**File:** `hosted/workers/tsconfig.json`

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022"],
    "types": ["@cloudflare/workers-types"],
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true
  },
  "include": ["src/**/*.ts"]
}
```

**File:** `hosted/workers/src/env.d.ts`

```typescript
export interface Env {
  DB: D1Database;
  CF_SECRET: string;
  POLAR_API_KEY: string;
  POLAR_WEBHOOK_SECRET: string;
  NOWPAYMENTS_API_KEY: string;
  NOWPAYMENTS_IPN_SECRET: string;
  CANARY_PUBLIC_KEY: string;
}
```

- [ ] Write all four files.
- [ ] Commit: `chore(hosted): workspace + workers package scaffold`.

### Phase B2: D1 schema

**File:** `hosted/workers/migrations/0001_init.sql`

Exact schema from spec §4.1. Includes indexes from spec §4.2.

- [ ] Write migration file.
- [ ] Commit: `feat(hosted): D1 schema migration 0001_init`.

### Phase B3: Crypto module (load-bearing)

**File:** `hosted/workers/src/crypto.ts`

XChaCha20-Poly1305 wrapper, HKDF, HMAC, K_worker derivation, key wrapping for `proxy_K_session`. All using `@noble/ciphers` and `@noble/hashes`.

- [ ] Write `crypto.ts` with full implementation (see spec §3.3-§3.5 for primitives).
- [ ] Write `crypto.test.ts` — roundtrip, nonce uniqueness, HMAC verify, K_session wrap/unwrap, **threat-model regression**: given (ciphertext, nonce, wrapped_proxy_K_session) without CF_SECRET, decryption must fail.
- [ ] Commit: `feat(hosted): crypto primitives + threat-model regression test`.

### Phase B4: DB + log + auth + quota modules

- [ ] `hosted/workers/src/db.ts` — typed prepared statements for each table.
- [ ] `hosted/workers/src/log.ts` — scrubbing logger; strips cookie names, base64 > 100 chars, `sk-alt-*`.
- [ ] `hosted/workers/src/auth.ts` — API key middleware (SHA256 hash lookup), session cookie verifier.
- [ ] `hosted/workers/src/quota.ts` — per-user/key concurrency + daily caps; uses Workers Analytics Engine for accounting.
- [ ] Commit: `feat(hosted): db, log, auth, quota`.

### Phase B5: Providers (hosted side, TS port of Python providers)

- [ ] `hosted/workers/src/providers/claude.ts` — TS port of `local/app/providers/claude.py`. Takes cookies + UA, returns AsyncIterable of `{delta: string}`.
- [ ] `hosted/workers/src/providers/chatgpt.ts` — TS port of `local/app/providers/chatgpt.py`.
- [ ] `hosted/workers/src/providers/gemini.ts` — TS port of `local/app/providers/gemini.py`.
- [ ] `hosted/workers/src/providers/index.ts` — `forModel(name)` routing.
- [ ] Commit: `feat(hosted): claude + chatgpt + gemini providers`.

### Phase B6: Routes

- [ ] `hosted/workers/src/routes/proxy.ts` — `/v1/chat/completions`, `/v1/models`. Loads ciphertext + `proxy_K_session`, derives K_worker per-request, decrypts in RAM, calls provider, streams SSE, logs usage row, zeroes secrets.
- [ ] `hosted/workers/src/routes/control.ts` — `/api/signup`, `/api/unlock`, `/api/keys/*`, `/api/account/status`.
- [ ] `hosted/workers/src/routes/sync.ts` — `/sync/upload`, `/sync/status`. HMAC verify before any write.
- [ ] `hosted/workers/src/routes/webhook.ts` — `/wh/polar`, `/wh/nowpayments`. Signature verify + idempotency.
- [ ] `hosted/workers/src/routes/canary.ts` — `/canary.txt`, ed25519-signed file from R2.
- [ ] `hosted/workers/src/index.ts` — Hono app routing by path prefix.
- [ ] Commit: `feat(hosted): routes (proxy, control, sync, webhook, canary)`.

### Phase B7: Billing integrations

- [ ] `hosted/workers/src/billing/polar.ts` — checkout creation + webhook signature verify (HMAC-SHA256 of body with shared secret).
- [ ] `hosted/workers/src/billing/nowpayments.ts` — invoice creation + IPN signature verify.
- [ ] Commit: `feat(hosted): polar + nowpayments billing`.

### Phase B8: Extension (MV3, Chrome + Firefox)

- [ ] `hosted/extension/manifest.json` — MV3 with minimum permissions.
- [ ] `hosted/extension/popup.html` + `popup.js` + `popup.css` — passphrase entry, status dots, recovery code display.
- [ ] `hosted/extension/background.js` — service worker; `cookies.onChanged` handler with 5s debounce; HMAC-signed POST.
- [ ] `hosted/extension/lib/crypto.js` — Argon2id (via `argon2-browser` WASM), XChaCha20-Poly1305, HKDF, HMAC-SHA256.
- [ ] `hosted/extension/lib/sync.js` — upload retries with jittered backoff.
- [ ] `hosted/extension/lib/state.js` — K_user RAM lifecycle, 5min idle lock.
- [ ] Commit: `feat(hosted): MV3 extension complete`.

### Phase B9: Dashboard (Astro static)

- [ ] `hosted/dashboard/package.json` + `astro.config.mjs` + `tsconfig.json`.
- [ ] `hosted/dashboard/src/pages/index.astro` — landing with threat model summary + extension install CTA.
- [ ] `hosted/dashboard/src/pages/app.astro` — authed dashboard (passphrase unlock → API key CRUD → status dots).
- [ ] `hosted/dashboard/src/pages/billing.astro` — Polar / NOWPayments checkout entry.
- [ ] `hosted/dashboard/src/pages/docs.astro` — endpoint reference.
- [ ] `hosted/dashboard/src/pages/canary.txt.ts` — proxies to worker `/canary.txt`.
- [ ] `hosted/dashboard/src/lib/crypto.ts` — same crypto primitives as extension (TypeScript port).
- [ ] `hosted/dashboard/src/lib/api.ts` — fetch wrappers.
- [ ] `hosted/dashboard/src/components/{StatusDots,KeyList,BillingPanel}.astro` — small components.
- [ ] Commit: `feat(hosted): Astro dashboard complete`.

### Phase B10: Ops + deployment docs

- [ ] `hosted/ops/canary.sh` — bash script: fetch latest BTC block hash, sign with ed25519 priv key (env var), upload to R2.
- [ ] `hosted/ops/deploy.md` — exact commands: `wrangler d1 create altkey`, `wrangler d1 migrations apply altkey`, `wrangler secret put CF_SECRET`, etc.
- [ ] `hosted/README.md` — high-level architecture link to spec + deploy quickstart.
- [ ] Commit: `feat(hosted): ops + deploy docs`.

### Phase B11: Top-level README

- [ ] `altkey/README.md` — explains the two tracks, links to `local/README.md` and `hosted/README.md` and the spec.
- [ ] Commit: `docs: top-level README pointing at both tracks`.

---

## Self-Review

**Spec coverage:** every numbered section in spec mapped to phases above:
- §1 architecture → B1, B6
- §2 components → B5, B6, B8, B9
- §3 crypto + threat model → B3 + test
- §4 data model → B2
- §5 data flows → B6, B7, B8
- §6 errors/abuse → B4 (quota, log), B6 (proxy errors)
- §7 testing → A4, B3 tests
- §8 sub-projects → Track A/B split mirrors this
- §10 runbook → B10

**Placeholder scan:** no TBD/TODO/"implement later" in actionable code blocks. Two acceptable placeholders in `wrangler.toml` (`REPLACE_WITH_YOUR_D1_ID`) and empty secrets (intentional — user supplies). Documented.

**Type consistency:** `K_user`, `K_session`, `K_worker`, `proxy_K_session`, `wrapped_K_session` used consistently across spec, plan, and code blocks. `sk-alt-*` is the only key prefix.

---

## Execution choice (per writing-plans skill)

User has pre-selected **Inline Execution** ("just finish the application"). Proceeding with `superpowers:executing-plans` batched execution: Track A first (so the local version is testable end-of-session), then Track B.
