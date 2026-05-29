"""ChatGPT via Codex OAuth → OpenAI Responses API (chatgpt.com/backend-api/codex).

Uses the ChatGPT subscription's Codex OAuth token (from ~/.codex/auth.json),
refreshing as needed. Supports chat, streaming, vision (image input), and tool
calling. Image generation is exposed separately via /v1/images/generations.
No cookies, no proof-of-work, no TLS impersonation.
"""
import json
import os
import time
import uuid
from pathlib import Path
from typing import AsyncIterator

import httpx

from .. import store

NAME = "chatgpt"
NATIVE = True  # exposes openai_completion / openai_stream (handles tool calls)

_RESPONSES = "https://chatgpt.com/backend-api/codex/responses"
_TOKEN_URL = "https://auth.openai.com/oauth/token"
_CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"  # public Codex CLI client id
_CODEX_CREDS = Path.home() / ".codex" / "auth.json"

# Models verified available on a Plus account via this endpoint.
MODELS = ["gpt-5.5", "gpt-5.4", "gpt-5.2"]
_DEFAULT = "gpt-5.5"

# requested name → real model. Anything unknown resolves to the frontier model.
_ALIASES = {
    "gpt-5.5": "gpt-5.5", "gpt-5.4": "gpt-5.4", "gpt-5.2": "gpt-5.2",
    "gpt-4o": _DEFAULT, "gpt-4o-mini": "gpt-5.4", "gpt-4.1": _DEFAULT,
    "gpt-5": _DEFAULT, "gpt-5-mini": "gpt-5.4", "chatgpt-4o-latest": _DEFAULT,
    "o3": _DEFAULT, "o4-mini": "gpt-5.4",
}


def _resolve_model(model: str) -> str:
    if not model:
        return _DEFAULT
    if model in MODELS:
        return model
    return _ALIASES.get(model.lower(), _DEFAULT)


def _headers(token: str, account_id: str) -> dict:
    return {
        "Authorization": f"Bearer {token}",
        "chatgpt-account-id": account_id,
        "OpenAI-Beta": "responses=experimental",
        "Content-Type": "application/json",
        "originator": "codex_cli_rs",
        "User-Agent": "codex_cli_rs/0.135.0",
    }


# ---- credentials / refresh ----------------------------------------------

def _read_codex_file() -> dict | None:
    try:
        if _CODEX_CREDS.exists():
            t = json.loads(_CODEX_CREDS.read_text())["tokens"]
            return {"refreshToken": t["refresh_token"], "accountId": t["account_id"]}
    except Exception:
        pass
    return None


def _creds() -> dict:
    c = store.load_session("chatgpt_oauth")
    if c and c.get("refreshToken"):
        return c
    seed = _read_codex_file()
    if not seed:
        raise RuntimeError("chatgpt (oauth) not connected — log into the Codex CLI, or seed the token")
    store.save_session("chatgpt_oauth", {"accessToken": "", "expiresAt": 0, **seed})
    return store.load_session("chatgpt_oauth")


async def _refresh(creds: dict) -> dict:
    async with httpx.AsyncClient(timeout=30) as c:
        r = await c.post(_TOKEN_URL, json={
            "grant_type": "refresh_token", "refresh_token": creds["refreshToken"],
            "client_id": _CLIENT_ID, "scope": "openid profile email offline_access",
        })
    if r.status_code >= 400:
        raise RuntimeError(f"chatgpt oauth refresh {r.status_code}: {r.text[:200]}")
    d = r.json()
    new = {
        "accessToken": d["access_token"],
        "refreshToken": d.get("refresh_token", creds["refreshToken"]),
        "accountId": creds["accountId"],
        "expiresAt": int((time.time() + d.get("expires_in", 3600)) * 1000),
    }
    store.save_session("chatgpt_oauth", new)
    return new


async def _token() -> tuple[str, str]:
    creds = _creds()
    if not creds.get("accessToken") or creds.get("expiresAt", 0) / 1000 <= time.time() + 60:
        creds = await _refresh(creds)
    return creds["accessToken"], creds["accountId"]


# ---- OpenAI <-> Responses translation ------------------------------------

def _content_to_responses(content) -> list:
    if isinstance(content, str):
        return [{"type": "input_text", "text": content}]
    out = []
    for part in content or []:
        if not isinstance(part, dict):
            continue
        if part.get("type") == "text":
            out.append({"type": "input_text", "text": part.get("text", "")})
        elif part.get("type") == "image_url":
            url = (part.get("image_url") or {}).get("url", "")
            out.append({"type": "input_image", "image_url": url})
    return out or [{"type": "input_text", "text": ""}]


def _build_input(messages: list[dict]) -> tuple[str, list]:
    """Returns (instructions, input_items). System messages become instructions."""
    instructions = []
    items = []
    for m in messages:
        role = m.get("role")
        if role == "system":
            c = m.get("content")
            instructions.append(c if isinstance(c, str) else "".join(
                p.get("text", "") for p in c if isinstance(p, dict)))
        elif role == "tool":
            items.append({"type": "function_call_output",
                          "call_id": m.get("tool_call_id", ""), "output": str(m.get("content", ""))})
        elif role == "assistant" and m.get("tool_calls"):
            for tc in m["tool_calls"]:
                fn = tc.get("function", {})
                items.append({"type": "function_call", "call_id": tc.get("id", ""),
                              "name": fn.get("name", ""), "arguments": fn.get("arguments", "{}")})
            if m.get("content"):
                items.append({"type": "message", "role": "assistant",
                              "content": [{"type": "output_text", "text": m["content"]}]})
        else:
            items.append({"type": "message", "role": role or "user",
                          "content": _content_to_responses(m.get("content"))})
    return ("\n\n".join(i for i in instructions if i) or "You are a helpful assistant.", items)


def _tools(openai_tools: list | None) -> list | None:
    if not openai_tools:
        return None
    out = []
    for t in openai_tools:
        if t.get("type") == "function":
            f = t["function"]
            out.append({"type": "function", "name": f["name"],
                        "description": f.get("description", ""),
                        "parameters": f.get("parameters", {"type": "object", "properties": {}})})
    return out or None


def _body(req: dict, stream: bool) -> tuple[dict, dict]:
    instructions, items = _build_input(req.get("messages", []))
    body = {"model": _resolve_model(req.get("model", _DEFAULT)),
            "instructions": instructions, "input": items,
            "stream": stream, "store": False}
    tools = _tools(req.get("tools"))
    if tools:
        body["tools"] = tools
    return body


def _chatcmpl_id() -> str:
    return f"chatcmpl-{uuid.uuid4().hex[:24]}"


async def openai_completion(req: dict) -> dict:
    # The Codex endpoint only supports stream=true, so we stream + aggregate.
    token, acct = await _token()
    body = _body(req, stream=True)
    text = ""
    tools_by_idx: dict[int, dict] = {}
    idx = -1
    usage = {}
    async with httpx.AsyncClient(timeout=httpx.Timeout(120, read=300)) as c:
        async with c.stream("POST", _RESPONSES, headers=_headers(token, acct), json=body) as r:
            if r.status_code >= 400:
                raise RuntimeError(f"chatgpt {r.status_code}: {(await r.aread())[:400].decode('utf-8','ignore')}")
            async for line in r.aiter_lines():
                line = line.strip()
                if not line.startswith("data:"):
                    continue
                try:
                    ev = json.loads(line[5:])
                except json.JSONDecodeError:
                    continue
                et = ev.get("type")
                if et == "response.output_text.delta":
                    text += ev.get("delta", "")
                elif et == "response.output_item.added" and (ev.get("item") or {}).get("type") == "function_call":
                    it = ev["item"]; idx += 1
                    tools_by_idx[idx] = {"id": it.get("call_id", ""), "name": it.get("name", ""), "args": ""}
                elif et == "response.function_call_arguments.delta":
                    if idx in tools_by_idx:
                        tools_by_idx[idx]["args"] += ev.get("delta", "")
                elif et == "response.completed":
                    usage = (ev.get("response") or {}).get("usage", {}) or {}
    tool_calls = [{"id": t["id"], "type": "function", "function": {"name": t["name"], "arguments": t["args"]}}
                  for t in tools_by_idx.values()]
    msg = {"role": "assistant", "content": text or None}
    if tool_calls:
        msg["tool_calls"] = tool_calls
    return {
        "id": _chatcmpl_id(), "object": "chat.completion", "created": int(time.time()),
        "model": req.get("model"),
        "choices": [{"index": 0, "message": msg, "finish_reason": "tool_calls" if tool_calls else "stop"}],
        "usage": {"prompt_tokens": usage.get("input_tokens", 0), "completion_tokens": usage.get("output_tokens", 0),
                  "total_tokens": usage.get("input_tokens", 0) + usage.get("output_tokens", 0)},
    }


async def openai_stream(req: dict) -> AsyncIterator[str]:
    token, acct = await _token()
    body = _body(req, stream=True)
    cid, created, model = _chatcmpl_id(), int(time.time()), req.get("model")

    def chunk(delta: dict, finish=None) -> str:
        return "data: " + json.dumps({"id": cid, "object": "chat.completion.chunk", "created": created,
                "model": model, "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}) + "\n\n"

    try:
        async with httpx.AsyncClient(timeout=httpx.Timeout(120, read=300)) as c:
            async with c.stream("POST", _RESPONSES, headers=_headers(token, acct), json=body) as r:
                if r.status_code >= 400:
                    err = await r.aread()
                    yield "data: " + json.dumps({"error": {"message": f"chatgpt {r.status_code}: {err[:300].decode('utf-8','ignore')}", "type": "upstream_error"}}) + "\n\n"
                    yield "data: [DONE]\n\n"
                    return
                yield chunk({"role": "assistant", "content": ""})
                tool_idx = -1
                finish = "stop"
                async for line in r.aiter_lines():
                    line = line.strip()
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if not data:
                        continue
                    try:
                        ev = json.loads(data)
                    except json.JSONDecodeError:
                        continue
                    et = ev.get("type")
                    if et == "response.output_text.delta":
                        yield chunk({"content": ev.get("delta", "")})
                    elif et == "response.output_item.added" and (ev.get("item") or {}).get("type") == "function_call":
                        it = ev["item"]
                        tool_idx += 1
                        finish = "tool_calls"
                        yield chunk({"tool_calls": [{"index": tool_idx, "id": it.get("call_id", ""), "type": "function",
                                    "function": {"name": it.get("name", ""), "arguments": ""}}]})
                    elif et == "response.function_call_arguments.delta":
                        yield chunk({"tool_calls": [{"index": tool_idx, "function": {"arguments": ev.get("delta", "")}}]})
                    elif et == "response.completed":
                        break
                yield chunk({}, finish)
                yield "data: [DONE]\n\n"
    except Exception as e:
        yield "data: " + json.dumps({"error": {"message": str(e), "type": "upstream_error"}}) + "\n\n"
        yield "data: [DONE]\n\n"


async def detect(session: dict) -> list[str]:
    store.save_detected_models("chatgpt", MODELS)
    return MODELS


# ---- image generation (used by /v1/images/generations) -------------------

async def generate_image(prompt: str, n: int = 1) -> list[str]:
    """Returns a list of base64 PNGs via the image_generation tool."""
    token, acct = await _token()
    body = {"model": _DEFAULT, "instructions": "You generate images when asked.",
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": prompt}]}],
            "tools": [{"type": "image_generation"}], "stream": True, "store": False}
    last_b64 = None
    async with httpx.AsyncClient(timeout=httpx.Timeout(180, read=300)) as c:
        async with c.stream("POST", _RESPONSES, headers=_headers(token, acct), json=body) as r:
            if r.status_code >= 400:
                raise RuntimeError(f"chatgpt image {r.status_code}: {(await r.aread())[:300]!r}")
            async for line in r.aiter_lines():
                line = line.strip()
                if not line.startswith("data:"):
                    continue
                try:
                    ev = json.loads(line[5:])
                except json.JSONDecodeError:
                    continue
                b = ev.get("partial_image_b64")
                if isinstance(b, str) and len(b) > 500:
                    last_b64 = b
    return [last_b64] if last_b64 else []
