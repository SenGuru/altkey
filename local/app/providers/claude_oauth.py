"""Claude via Claude Code OAuth token → the real Anthropic Messages API.

Unlike the chat-backend relay, this hits api.anthropic.com directly using the
subscription's OAuth token, so tool calling, streaming, vision, and structured
output all work natively. Requires the "oauth" beta header and the Claude Code
attestation system prompt.
"""
import json
import time
import uuid
from pathlib import Path
from typing import AsyncIterator

import httpx

from .. import store

_LIVE_CREDS = Path.home() / ".claude" / ".credentials.json"

NAME = "claude"
NATIVE = True  # exposes openai_stream/openai_completion (handles tool calls)

_API = "https://api.anthropic.com/v1/messages"
_MODELS_API = "https://api.anthropic.com/v1/models"
_TOKEN_API = "https://console.anthropic.com/v1/oauth/token"
# Public Claude Code OAuth client id (used only to authorize the user's own sub).
_CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
_ATTEST = "You are Claude Code, Anthropic's official CLI for Claude."
_BETA = "oauth-2025-04-20"

# OAuth "Connect with Claude" flow (PKCE) — matches Claude Code's real params:
# loopback redirect (altkey catches it → zero paste) + single user:inference scope.
_AUTHORIZE_URL = "https://claude.ai/oauth/authorize"
_SCOPES = "user:inference"


def _redirect_uri() -> str:
    # Path MUST be exactly /callback — Claude validates the loopback redirect
    # path strictly (it's what `claude setup-token` registers).
    import os as _os
    port = _os.environ.get("ALTKEY_PORT", "8787")
    return f"http://localhost:{port}/callback"


def _b64url(data: bytes) -> str:
    import base64
    return base64.urlsafe_b64encode(data).decode().rstrip("=")


def start_oauth() -> dict:
    """Begin the Connect-with-Claude flow. Returns the authorize URL the user
    opens; stashes the PKCE verifier so finish_oauth() can complete it."""
    import hashlib
    import os as _os
    from urllib.parse import urlencode, quote
    verifier = _b64url(_os.urandom(32))
    challenge = _b64url(hashlib.sha256(verifier.encode()).digest())
    state = _b64url(_os.urandom(32))  # 32 bytes → 43 chars, matching setup-token
    store.save_session("_oauth_pkce", {"verifier": verifier, "state": state, "ts": int(time.time())})
    params = {
        "code": "true",
        "client_id": _CLIENT_ID,
        "response_type": "code",
        "redirect_uri": _redirect_uri(),
        "scope": _SCOPES,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
        "state": state,
    }
    return {"url": f"{_AUTHORIZE_URL}?{urlencode(params, quote_via=quote)}"}


async def finish_oauth(code_input: str) -> dict:
    """Complete the flow with the code the user pasted (format: CODE#STATE)."""
    pkce = store.load_session("_oauth_pkce")
    if not pkce:
        raise RuntimeError("no pending OAuth flow — click Connect Claude first")
    code = code_input.strip()
    state = pkce["state"]
    if "#" in code:
        code, state = code.split("#", 1)
    async with httpx.AsyncClient(timeout=30) as c:
        r = await c.post(_TOKEN_API, json={
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": _redirect_uri(),
            "client_id": _CLIENT_ID,
            "code_verifier": pkce["verifier"],
            "state": state,
        })
    if r.status_code >= 400:
        raise RuntimeError(f"oauth exchange {r.status_code}: {r.text[:300]}")
    d = r.json()
    store.save_session("claude_oauth", {
        "accessToken": d["access_token"],
        "refreshToken": d.get("refresh_token", ""),
        "expiresAt": int((time.time() + d.get("expires_in", 28800)) * 1000),
        "subscriptionType": (d.get("account") or {}).get("subscription_type", "unknown"),
    })
    store.delete_session("_oauth_pkce")
    return {"ok": True, "subscription": (d.get("account") or {}).get("subscription_type", "unknown")}

MODELS = [
    "claude-opus-4-8",
    "claude-opus-4-5",
    "claude-sonnet-4-6",
    "claude-haiku-4-5",
    "claude-3-7-sonnet",
    "claude-3-5-sonnet",
    "claude-3-5-haiku",
]


def _headers(token: str) -> dict:
    return {
        "authorization": f"Bearer {token}",
        "anthropic-version": "2023-06-01",
        "anthropic-beta": _BETA,
        "content-type": "application/json",
        "user-agent": "claude-cli/1.0.0 (external)",
        "x-app": "cli",
    }


# ---- credentials / refresh ----------------------------------------------

def _read_live() -> dict | None:
    """Read the live Claude Code credentials (kept fresh by Claude Code itself).
    Preferred locally — avoids refresh-token rotation conflicts."""
    try:
        if _LIVE_CREDS.exists():
            return json.loads(_LIVE_CREDS.read_text()).get("claudeAiOauth")
    except Exception:
        pass
    return None


import os


def seed_from_env() -> bool:
    """On a headless server there's no ~/.claude file. Seed the OAuth token once
    from the ALTKEY_CLAUDE_OAUTH_JSON secret (the claudeAiOauth object as JSON).
    Only seeds if we don't already have a stored session (so the server keeps
    its own refreshed lineage across restarts when storage persists)."""
    if store.load_session("claude_oauth"):
        return False
    raw = os.environ.get("ALTKEY_CLAUDE_OAUTH_JSON")
    if not raw:
        return False
    try:
        d = json.loads(raw)
        d = d.get("claudeAiOauth", d)  # accept either the full file or the inner object
        store.save_session("claude_oauth", {
            "accessToken": d["accessToken"],
            "refreshToken": d.get("refreshToken", ""),
            "expiresAt": d.get("expiresAt", 0),
            "subscriptionType": d.get("subscriptionType", "unknown"),
        })
        return True
    except Exception:
        return False


async def refresh_loop():
    """Proactively refresh the OAuth token before it expires so it never lapses
    on an idle server. Runs forever; no-ops when not connected."""
    import asyncio
    while True:
        try:
            creds = store.load_session("claude_oauth")
            if creds:
                # refresh ~10 min before expiry (or now if already past)
                wait = max(0, creds.get("expiresAt", 0) / 1000 - time.time() - 600)
                await asyncio.sleep(min(wait, 3600))
                creds = store.load_session("claude_oauth")
                if creds and creds.get("expiresAt", 0) / 1000 <= time.time() + 600:
                    await _refresh(creds)
            else:
                await asyncio.sleep(300)
        except Exception:
            await asyncio.sleep(300)


async def _token() -> str:
    # 1) Prefer the live Claude Code file if its token is still valid.
    live = _read_live()
    if live and live.get("expiresAt", 0) / 1000 > time.time() + 60:
        store.save_session("claude_oauth", {
            "accessToken": live["accessToken"],
            "refreshToken": live.get("refreshToken", ""),
            "expiresAt": live.get("expiresAt", 0),
            "subscriptionType": live.get("subscriptionType", "unknown"),
        })
        return live["accessToken"]

    # 2) Fall back to our stored copy, refreshing if needed.
    creds = store.load_session("claude_oauth")
    if not creds:
        raise RuntimeError("claude (oauth) not connected — run Connect Claude (CLI)")
    if creds.get("expiresAt", 0) / 1000 <= time.time() + 60:
        # If the live file has a (newer) refresh token, prefer it.
        if live and live.get("refreshToken"):
            creds = dict(creds, refreshToken=live["refreshToken"])
        creds = await _refresh(creds)
    return creds["accessToken"]


async def _refresh(creds: dict) -> dict:
    rt = creds.get("refreshToken")
    if not rt:
        raise RuntimeError("claude oauth token expired and no refresh token")
    async with httpx.AsyncClient(timeout=30) as c:
        r = await c.post(_TOKEN_API, json={
            "grant_type": "refresh_token",
            "refresh_token": rt,
            "client_id": _CLIENT_ID,
        })
    if r.status_code >= 400:
        raise RuntimeError(f"claude oauth refresh {r.status_code}: {r.text[:200]}")
    d = r.json()
    new = {
        "accessToken": d["access_token"],
        "refreshToken": d.get("refresh_token", rt),
        "expiresAt": int((time.time() + d.get("expires_in", 3600)) * 1000),
        "subscriptionType": creds.get("subscriptionType", "unknown"),
    }
    store.save_session("claude_oauth", new)
    return new


# ---- model resolution ----------------------------------------------------

_OPUS = "claude-opus-4-5-20251101"
_SONNET = "claude-sonnet-4-5-20250929"
_HAIKU = "claude-haiku-4-5-20251001"


def _resolve_model(model: str) -> str:
    detected = store.load_detected_models("claude_oauth")
    ids = detected["models"] if detected else []
    m = (model or "").lower()
    fam = "opus" if "opus" in m else "haiku" if "haiku" in m else "sonnet"
    # exact id present in detected list → honor
    if model in ids:
        return model
    matches = [x for x in ids if fam in x]
    if matches:
        return matches[0]
    return {"opus": _OPUS, "sonnet": _SONNET, "haiku": _HAIKU}[fam]


async def detect(session: dict) -> list[str]:
    token = await _token()
    async with httpx.AsyncClient(timeout=30) as c:
        r = await c.get(_MODELS_API, headers=_headers(token))
    if r.status_code >= 400:
        return []
    ids = [m["id"] for m in r.json().get("data", []) if m.get("id")]
    # newest-first by id (dates sort lexically within a family well enough)
    ids.sort(reverse=True)
    if ids:
        store.save_detected_models("claude_oauth", ids)
    return ids


# ---- OpenAI <-> Anthropic translation -------------------------------------

def _content_to_anthropic(content) -> list:
    if isinstance(content, str):
        return [{"type": "text", "text": content}]
    blocks = []
    for part in content or []:
        if not isinstance(part, dict):
            continue
        t = part.get("type")
        if t == "text":
            blocks.append({"type": "text", "text": part.get("text", "")})
        elif t == "image_url":
            url = (part.get("image_url") or {}).get("url", "")
            if url.startswith("data:"):
                header, b64 = url.split(",", 1)
                media = header[5:].split(";")[0] or "image/png"
                blocks.append({"type": "image", "source": {"type": "base64", "media_type": media, "data": b64}})
    return blocks or [{"type": "text", "text": ""}]


def _to_anthropic(req: dict) -> dict:
    system_blocks = [{"type": "text", "text": _ATTEST}]
    messages = []
    for m in req.get("messages", []):
        role = m.get("role")
        if role == "system":
            txt = m.get("content")
            if isinstance(txt, list):
                txt = "".join(p.get("text", "") for p in txt if isinstance(p, dict))
            system_blocks.append({"type": "text", "text": str(txt)})
        elif role == "tool":
            messages.append({"role": "user", "content": [{
                "type": "tool_result",
                "tool_use_id": m.get("tool_call_id", ""),
                "content": m.get("content", ""),
            }]})
        elif role == "assistant" and m.get("tool_calls"):
            blocks = []
            if m.get("content"):
                blocks.append({"type": "text", "text": m["content"]})
            for tc in m["tool_calls"]:
                fn = tc.get("function", {})
                try:
                    args = json.loads(fn.get("arguments") or "{}")
                except json.JSONDecodeError:
                    args = {}
                blocks.append({"type": "tool_use", "id": tc.get("id", ""), "name": fn.get("name", ""), "input": args})
            messages.append({"role": "assistant", "content": blocks})
        else:
            messages.append({"role": role, "content": _content_to_anthropic(m.get("content"))})

    body = {
        "model": _resolve_model(req.get("model", "claude-sonnet-4-6")),
        "max_tokens": req.get("max_tokens") or 8192,
        "system": system_blocks,
        "messages": messages,
    }
    if req.get("temperature") is not None:
        body["temperature"] = req["temperature"]
    if req.get("tools"):
        body["tools"] = [{
            "name": t["function"]["name"],
            "description": t["function"].get("description", ""),
            "input_schema": t["function"].get("parameters", {"type": "object", "properties": {}}),
        } for t in req["tools"] if t.get("type") == "function"]
    tc = req.get("tool_choice")
    if tc == "required":
        body["tool_choice"] = {"type": "any"}
    elif isinstance(tc, dict) and tc.get("function"):
        body["tool_choice"] = {"type": "tool", "name": tc["function"]["name"]}
    return body


def _chatcmpl_id() -> str:
    return f"chatcmpl-{uuid.uuid4().hex[:24]}"


async def openai_completion(req: dict) -> dict:
    token = await _token()
    body = _to_anthropic(req)
    async with httpx.AsyncClient(timeout=httpx.Timeout(120, read=300)) as c:
        r = await c.post(_API, headers=_headers(token), json=body)
    if r.status_code >= 400:
        raise RuntimeError(f"claude oauth {r.status_code}: {r.text[:400]}")
    d = r.json()
    text = "".join(b.get("text", "") for b in d.get("content", []) if b.get("type") == "text")
    tool_calls = []
    for b in d.get("content", []):
        if b.get("type") == "tool_use":
            tool_calls.append({
                "id": b.get("id", ""),
                "type": "function",
                "function": {"name": b.get("name", ""), "arguments": json.dumps(b.get("input", {}))},
            })
    msg = {"role": "assistant", "content": text or None}
    if tool_calls:
        msg["tool_calls"] = tool_calls
    finish = "tool_calls" if tool_calls else "stop"
    usage = d.get("usage", {})
    return {
        "id": _chatcmpl_id(),
        "object": "chat.completion",
        "created": int(time.time()),
        "model": req.get("model"),
        "choices": [{"index": 0, "message": msg, "finish_reason": finish}],
        "usage": {
            "prompt_tokens": usage.get("input_tokens", 0),
            "completion_tokens": usage.get("output_tokens", 0),
            "total_tokens": usage.get("input_tokens", 0) + usage.get("output_tokens", 0),
        },
    }


def _inject_attestation(body: dict) -> dict:
    """Anthropic-native passthrough: prepend the Claude Code attestation block
    to whatever system prompt the client sent (required for oauth tokens)."""
    body = dict(body)
    sys = body.get("system")
    attest = {"type": "text", "text": _ATTEST}
    if sys is None:
        body["system"] = [attest]
    elif isinstance(sys, str):
        body["system"] = [attest, {"type": "text", "text": sys}]
    elif isinstance(sys, list):
        if not (sys and isinstance(sys[0], dict) and sys[0].get("text", "").startswith("You are Claude Code")):
            body["system"] = [attest, *sys]
    body["model"] = _resolve_model(body.get("model", "claude-sonnet-4-6"))
    return body


async def anthropic_messages(body: dict):
    """Native Anthropic /v1/messages passthrough (non-streaming). Returns the
    raw Anthropic JSON so clients using the Anthropic SDK work unchanged."""
    token = await _token()
    fwd = _inject_attestation(body)
    fwd.pop("stream", None)
    async with httpx.AsyncClient(timeout=httpx.Timeout(120, read=300)) as c:
        r = await c.post(_API, headers=_headers(token), json=fwd)
    return r.status_code, r.json() if r.headers.get("content-type", "").startswith("application/json") else {"error": r.text[:400]}


async def anthropic_messages_stream(body: dict) -> AsyncIterator[str]:
    """Native Anthropic /v1/messages passthrough (streaming) — forwards the raw
    Anthropic SSE event stream unchanged."""
    token = await _token()
    fwd = dict(_inject_attestation(body), stream=True)
    async with httpx.AsyncClient(timeout=httpx.Timeout(120, read=300)) as c:
        async with c.stream("POST", _API, headers=_headers(token), json=fwd) as r:
            if r.status_code >= 400:
                err = await r.aread()
                yield f"event: error\ndata: {json.dumps({'type':'error','error':{'message':err[:300].decode('utf-8','ignore')}})}\n\n"
                return
            async for line in r.aiter_raw():
                yield line.decode("utf-8", "ignore")


async def openai_stream(req: dict) -> AsyncIterator[str]:
    token = await _token()
    body = dict(_to_anthropic(req), stream=True)
    cid = _chatcmpl_id()
    created = int(time.time())
    model = req.get("model")

    def chunk(delta: dict, finish=None) -> str:
        return "data: " + json.dumps({
            "id": cid, "object": "chat.completion.chunk", "created": created, "model": model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        }) + "\n\n"

    try:
        async with httpx.AsyncClient(timeout=httpx.Timeout(120, read=300)) as c:
            async with c.stream("POST", _API, headers=_headers(token), json=body) as r:
                if r.status_code >= 400:
                    err = await r.aread()
                    yield "data: " + json.dumps({"error": {"message": f"claude oauth {r.status_code}: {err[:300].decode('utf-8','ignore')}", "type": "upstream_error"}}) + "\n\n"
                    yield "data: [DONE]\n\n"
                    return
                yield chunk({"role": "assistant", "content": ""})
                tool_idx = -1
                tool_id = ""
                tool_name = ""
                finish = "stop"
                async for line in r.aiter_lines():
                    line = line.strip()
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if not data:
                        continue
                    try:
                        evt = json.loads(data)
                    except json.JSONDecodeError:
                        continue
                    et = evt.get("type")
                    if et == "content_block_start":
                        blk = evt.get("content_block", {})
                        if blk.get("type") == "tool_use":
                            tool_idx += 1
                            tool_id = blk.get("id", "")
                            tool_name = blk.get("name", "")
                            finish = "tool_calls"
                            yield chunk({"tool_calls": [{"index": tool_idx, "id": tool_id, "type": "function",
                                        "function": {"name": tool_name, "arguments": ""}}]})
                    elif et == "content_block_delta":
                        d = evt.get("delta", {})
                        if d.get("type") == "text_delta":
                            yield chunk({"content": d.get("text", "")})
                        elif d.get("type") == "input_json_delta":
                            yield chunk({"tool_calls": [{"index": tool_idx, "function": {"arguments": d.get("partial_json", "")}}]})
                    elif et == "message_delta":
                        sr = evt.get("delta", {}).get("stop_reason")
                        if sr == "tool_use":
                            finish = "tool_calls"
                    elif et == "message_stop":
                        break
                yield chunk({}, finish)
                yield "data: [DONE]\n\n"
    except Exception as e:
        yield "data: " + json.dumps({"error": {"message": str(e), "type": "upstream_error"}}) + "\n\n"
        yield "data: [DONE]\n\n"
