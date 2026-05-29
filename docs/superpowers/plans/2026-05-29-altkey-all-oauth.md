# altkey All-OAuth (Phase 0, Python) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make altkey all-OAuth in Python — ChatGPT via Codex OAuth, Gemini via CLI OAuth (Claude OAuth already works) — and delete the cookie relay, proof-of-work, TLS impersonation, and browser extension. Result: a clean all-OAuth Python backend that is the reference for the later Rust port.

**Architecture:** Each provider becomes a direct OAuth call to its real API (no cookies, no impersonation). ChatGPT/Gemini providers are modeled on the proven `claude_oauth.py`. Discovery-heavy parts (exact Codex/Gemini request shapes) are explicit spikes before the build.

**Tech Stack:** Python 3.11+, FastAPI, httpx (no more curl_cffi), existing store/oauth patterns.

**Branch:** `main`. **Phase 1 (Rust) is a separate plan, written after this completes.**

---

## Honest note on TDD here
The provider request shapes for ChatGPT-Codex and Gemini-CLI are **reverse-engineering unknowns**. You cannot write a passing unit test for a request shape you haven't discovered. So each new provider starts with a **spike task** (a throwaway script that hits the real API and records what works), and only then do the build + unit tests (which test the *translation* logic, the part that's deterministic). Live smoke tests are the real acceptance check, run manually.

---

## File structure (Phase 0)

```
local/app/providers/
  claude_oauth.py      KEEP (proven reference template)
  chatgpt.py           REWRITE → Codex OAuth (was cookie+PoW)
  gemini.py            REWRITE → CLI OAuth (was cookie+StreamGenerate)
  claude.py            DELETE (legacy chat relay; claude_oauth replaces it)
  __init__.py          MODIFY (routing: all OAuth)
  _imgutil.py          KEEP (image extraction still used for vision)
local/app/
  main.py              MODIFY (remove /admin/capture, /admin/import; keep OAuth + connect-cli)
  harvester.py         DELETE (Playwright cookie capture — gone)
  store.py             KEEP (sessions/keys/detected-models)
local/extension/       DELETE (cookie capture extension — obsolete)
local/tests/
  test_providers_translate.py  MODIFY (drop cookie-relay assertions)
  test_harvester_smoke.py      DELETE (harvester gone)
  test_store.py                KEEP
```

---

## Task 0.1: Spike — ChatGPT Codex OAuth (discover the working request)

**Files:** Create (throwaway): `local/spikes/codex_spike.py`

- [ ] **Step 1: Read the Codex token + probe the API**

```python
# local/spikes/codex_spike.py
import json, httpx
auth = json.load(open(r"C:/Users/gsent/.codex/auth.json"))
tok = auth["tokens"]["access_token"]; acct = auth["tokens"]["account_id"]
print("token prefix:", tok[:12], "| account:", acct)

# Codex hits the Responses API on chatgpt.com with the OAuth bearer.
headers = {
    "Authorization": f"Bearer {tok}",
    "chatgpt-account-id": acct,
    "OpenAI-Beta": "responses=experimental",
    "Content-Type": "application/json",
    "originator": "codex_cli_rs",
    "User-Agent": "codex_cli_rs",
}
body = {
    "model": "gpt-5",
    "instructions": "You are a helpful assistant.",
    "input": [{"type":"message","role":"user","content":[{"type":"input_text","text":"Reply with exactly: codex works"}]}],
    "stream": True,
    "store": False,
}
with httpx.Client(timeout=60) as c:
    r = c.post("https://chatgpt.com/backend-api/codex/responses", headers=headers, json=body)
    print("status:", r.status_code)
    print(r.text[:800])
```

- [ ] **Step 2: Run it, record findings**

Run: `python local/spikes/codex_spike.py`
Record: the working endpoint, required headers, request body shape, the SSE event
format of the response, and whether `model` accepts `gpt-5`. If 401 → refresh the
token (Codex refresh flow) and note it. If a different endpoint/header is needed,
record the corrected version. **This recorded shape is the input to Task 0.2.**

- [ ] **Step 3: Commit the spike findings as a comment block**

```bash
git add local/spikes/codex_spike.py
git commit -m "spike: discover ChatGPT Codex OAuth request shape"
```

---

## Task 0.2: ChatGPT provider on Codex OAuth

**Files:**
- Rewrite: `local/app/providers/chatgpt.py`
- Test: `local/tests/test_providers_translate.py` (translation units)

- [ ] **Step 1: Write the translation unit test (deterministic part)**

```python
# in test_providers_translate.py
def test_chatgpt_to_responses_input():
    from app.providers import chatgpt
    parts = chatgpt._to_input([{"role":"user","content":"hi"}])
    assert parts[0]["role"] == "user"
    assert parts[0]["content"][0]["text"] == "hi"
```

- [ ] **Step 2: Run it, expect fail**

Run: `python -m pytest local/tests/test_providers_translate.py::test_chatgpt_to_responses_input -v`
Expected: FAIL (function not defined / module rewritten)

- [ ] **Step 3: Rewrite chatgpt.py using the spike's discovered shape**

Model it on `claude_oauth.py`. Structure (fill request shape from Task 0.1 findings):

```python
import json, time, uuid
from typing import AsyncIterator
import httpx
from .. import store

NAME = "chatgpt"
NATIVE = True
MODELS = ["gpt-5", "gpt-5-mini", "gpt-5-thinking", "o3"]  # refine from /backend-api/models
_RESPONSES = "https://chatgpt.com/backend-api/codex/responses"  # confirm in spike

def _creds() -> dict:
    c = store.load_session("chatgpt_oauth")
    if not c:
        raise RuntimeError("chatgpt (oauth) not connected")
    return c

def _headers(c: dict) -> dict:
    return {
        "Authorization": f"Bearer {c['accessToken']}",
        "chatgpt-account-id": c["accountId"],
        "OpenAI-Beta": "responses=experimental",
        "Content-Type": "application/json",
        "originator": "codex_cli_rs",
    }

def _to_input(messages: list[dict]) -> list[dict]:
    out = []
    for m in messages:
        content = m.get("content")
        text = content if isinstance(content, str) else "".join(
            p.get("text","") for p in content if isinstance(p, dict) and p.get("type")=="text")
        out.append({"type":"message","role":m.get("role","user"),
                    "content":[{"type":"input_text","text":text}]})
    return out

# openai_completion / openai_stream that POST to _RESPONSES, parse the SSE
# 'response.output_text.delta' events into OpenAI chunks, and map tool calls.
# (Exact event names from the spike.)
```

- [ ] **Step 4: Run translation test, expect pass**

Run: `python -m pytest local/tests/test_providers_translate.py::test_chatgpt_to_responses_input -v`
Expected: PASS

- [ ] **Step 5: Live smoke test**

Run a manual completion through `/v1/chat/completions` with `model=gpt-5` after
seeding `chatgpt_oauth` from `~/.codex/auth.json`. Expected: real reply + a tool call.

- [ ] **Step 6: Commit**

```bash
git add local/app/providers/chatgpt.py local/tests/test_providers_translate.py
git commit -m "feat(local): ChatGPT via Codex OAuth (Responses API)"
```

---

## Task 0.3: Spike — Gemini CLI OAuth (discover)

**Files:** Create (throwaway): `local/spikes/gemini_spike.py`

- [ ] **Step 1: Locate Gemini CLI creds + probe**

The Gemini CLI stores OAuth creds (Google) under `~/.gemini/` (e.g.
`oauth_creds.json`) once the user runs `gemini` and logs in. Probe the Code
Assist / Gemini API with the bearer:

```python
# local/spikes/gemini_spike.py
import json, glob, httpx, os
path = os.path.expanduser("~/.gemini/oauth_creds.json")
creds = json.load(open(path))
tok = creds.get("access_token")
print("token prefix:", (tok or "")[:12])
headers = {"Authorization": f"Bearer {tok}", "Content-Type": "application/json"}
# Gemini CLI uses Cloud Code Assist; confirm endpoint + body in the spike.
body = {"contents":[{"role":"user","parts":[{"text":"Reply with exactly: gemini works"}]}]}
with httpx.Client(timeout=60) as c:
    r = c.post("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent",
               headers=headers, json=body)
    print("status:", r.status_code); print(r.text[:800])
```

- [ ] **Step 2: Run, record findings**

Run: `python local/spikes/gemini_spike.py`
Record the working endpoint (generativelanguage vs cloudcode), the body shape,
the streaming variant (`streamGenerateContent`), and the response JSON path for
text. If the user hasn't logged into Gemini CLI, note: "run `gemini` and log in
first." **Recorded shape feeds Task 0.4.**

- [ ] **Step 3: Commit**

```bash
git add local/spikes/gemini_spike.py
git commit -m "spike: discover Gemini CLI OAuth request shape"
```

---

## Task 0.4: Gemini provider on CLI OAuth

**Files:**
- Rewrite: `local/app/providers/gemini.py`
- Test: `local/tests/test_providers_translate.py`

- [ ] **Step 1: Translation unit test**

```python
def test_gemini_to_contents():
    from app.providers import gemini
    contents = gemini._to_contents([{"role":"user","content":"hi"}])
    assert contents[0]["parts"][0]["text"] == "hi"
    assert contents[0]["role"] == "user"
```

- [ ] **Step 2: Run, expect fail**

Run: `python -m pytest local/tests/test_providers_translate.py::test_gemini_to_contents -v`
Expected: FAIL

- [ ] **Step 3: Rewrite gemini.py (Gemini API generateContent, from spike)**

```python
NAME = "gemini"; NATIVE = True
MODELS = ["gemini-2.5-pro","gemini-2.5-flash","gemini-2.0-flash"]
def _to_contents(messages):
    out=[]
    for m in messages:
        role = "model" if m.get("role")=="assistant" else "user"
        content=m.get("content")
        text=content if isinstance(content,str) else "".join(
            p.get("text","") for p in content if isinstance(p,dict) and p.get("type")=="text")
        out.append({"role":role,"parts":[{"text":text}]})
    return out
# openai_completion / openai_stream: POST generateContent / streamGenerateContent,
# map candidates[0].content.parts[].text → OpenAI chunks; map functionCall → tool_calls.
```

- [ ] **Step 4: Run test, expect pass**

Run: `python -m pytest local/tests/test_providers_translate.py::test_gemini_to_contents -v`
Expected: PASS

- [ ] **Step 5: Live smoke test** (completion + tool call via `/v1/chat/completions`, `model=gemini-2.5-flash`)

- [ ] **Step 6: Commit**

```bash
git add local/app/providers/gemini.py local/tests/test_providers_translate.py
git commit -m "feat(local): Gemini via CLI OAuth (generateContent)"
```

---

## Task 0.5: Remove cookie machinery

**Files:**
- Delete: `local/app/providers/claude.py`, `local/app/harvester.py`, `local/extension/` (whole dir), `local/tests/test_harvester_smoke.py`
- Modify: `local/app/main.py`, `local/app/providers/__init__.py`, `local/pyproject.toml`

- [ ] **Step 1: Delete the files**

```bash
git rm local/app/providers/claude.py local/app/harvester.py local/tests/test_harvester_smoke.py
git rm -r local/extension
```

- [ ] **Step 2: Remove endpoints from main.py**

Delete the `/admin/capture`, `/admin/import`, `/admin/connect` (Playwright) handlers and the `harvest` import and the `_IMPORT_*`/`CaptureReq`/`ImportReq` blocks. Keep `/admin/connect-cli`, `/admin/oauth/*`, `/callback`, `/v1/*`, `/admin/keys*`, `/admin/status`, `/admin/detect`, `/admin/disconnect`.

- [ ] **Step 3: Update providers/__init__.py routing**

```python
from . import claude_oauth, chatgpt, gemini
from .. import store

def for_model(model: str):
    m = model.lower()
    if m.startswith("claude"): return claude_oauth
    if m.startswith(("gpt","o1","o3","o4","chatgpt")): return chatgpt
    if m.startswith("gemini"): return gemini
    return None
```
(Remove the chat-relay `claude` and the `_claude_module()` fallback.)

- [ ] **Step 4: Drop curl_cffi from pyproject.toml**

Remove the `"curl_cffi>=0.7",` line (no impersonation needed anymore).

- [ ] **Step 5: Run full test suite**

Run: `python -m pytest local/tests/ -v`
Expected: PASS (after Task 0.6 fixes the remaining cookie-coupled tests).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(local): remove cookie relay, harvester, extension, PoW, impersonation"
```

---

## Task 0.6: Fix the test suite for all-OAuth

**Files:** Modify `local/tests/test_providers_translate.py`

- [ ] **Step 1: Remove cookie-relay assertions**

Delete tests referencing the old `claude` chat-relay module, `_flatten`/cookie
helpers, ChatGPT `_to_parts`/`_device_id`, Gemini `_MODEL_HEADER`, and harvester.
Keep store tests + the new translation tests (Tasks 0.2/0.4) + routing tests
(now expecting `claude_oauth`/`chatgpt`/`gemini`).

- [ ] **Step 2: Run suite, expect green**

Run: `python -m pytest local/tests/ -v`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add local/tests/
git commit -m "test(local): all-OAuth test suite (drop cookie tests)"
```

---

## Task 0.7: End-to-end validation + connect flows

- [ ] **Step 1: Add ChatGPT + Gemini connect** (mirror Claude's `connect-cli`/OAuth)

Add `/admin/connect-cli` handling for `chatgpt` (read `~/.codex/auth.json` →
`chatgpt_oauth` session) and `gemini` (read `~/.gemini/oauth_creds.json` →
`gemini_oauth` session), plus refresh handling per the spikes.

- [ ] **Step 2: Live smoke all three** (completion, stream, tool call, vision each)

Run the server, connect all three, fire the four request types per provider.
Expected: all succeed, matching prior Claude behavior.

- [ ] **Step 3: Commit + tag the reference**

```bash
git add -A
git commit -m "feat(local): all-OAuth backend complete — reference for Rust port"
git tag all-oauth-python-reference
```

---

## Phase 1 (Rust) — OUTLINE ONLY (separate plan after Phase 0)

Once Phase 0 is committed + tagged, write a dedicated Rust plan covering the
spec §4 modules (`main, config, auth, store, oauth, translate, sse, models, log,
providers/{claude,chatgpt,gemini}`) on the `dev` branch, **based on `main` after
Phase 0**. It will:
- Capture parity fixtures from the tagged Python reference.
- Port module-by-module (axum + reqwest + tokio + sqlx), each validated against
  its fixture.
- Live smoke tests per provider.
- Reuse `dashboard.html` + transparent scripts unchanged.

Phase 1 is intentionally not detailed here because its exact code depends on the
proven Phase-0 Python, which does not exist until this plan completes.

---

## Self-Review

**Spec coverage:** Phase 0 covers spec §0/§1/§2 (all-OAuth, drop cookies/extension/PoW/impersonation, three OAuth providers, parity of non-cookie features) and §9 Phase-0 DoD. Phase 1 (spec §3/§4/§5) is deferred to its own plan, as the spec's two-phase §1 sequencing requires.

**Placeholder scan:** The provider request shapes are intentionally discovered in spike tasks (0.1, 0.3) before the build tasks (0.2, 0.4) — this is the honest structure for reverse-engineering, not a placeholder. Build-task code shows the deterministic translation + provider skeleton; the request/SSE specifics are filled from the immediately-preceding spike (same task group, not "later").

**Type consistency:** session names `chatgpt_oauth`/`gemini_oauth`/`claude_oauth`; provider `NAME` = `chatgpt`/`gemini`/`claude`; `NATIVE=True` providers expose `openai_completion`/`openai_stream` (matching the existing claude_oauth interface in main.py).
