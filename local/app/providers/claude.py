import json
import time
import uuid
from typing import AsyncIterator

from curl_cffi.requests import AsyncSession

from .. import store

NAME = "claude"
MODELS = [
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
    "claude-3-5-sonnet-20241022",
    "claude-3-5-haiku-20241022",
]

_BASE = "https://claude.ai"
_DOMAINS = ("claude.ai",)
_IMPERSONATE = "chrome"

# claude.ai web backend uses its own model identifiers, distinct from the
# public API names, and availability varies per account/plan. Map our
# OpenAI-style aliases to web identifiers verified against a live account.
# If a request hits model_not_available, stream() auto-retries with the model
# field omitted so the account default is used — never a hard failure.
_WEB_MODEL = {
    "claude-opus-4-5": "claude-opus-4-5",
    "claude-sonnet-4-5": "claude-sonnet-4-20250514",
    "claude-sonnet-4": "claude-sonnet-4-20250514",
    "claude-haiku-4-5": "claude-haiku-4-5",
}


class _ModelNotAvailable(Exception):
    pass


def _cookies(session: dict) -> dict:
    out = {}
    for c in session.get("cookies", []):
        dom = (c.get("domain") or "").lstrip(".")
        if any(dom.endswith(d) for d in _DOMAINS):
            out[c["name"]] = c["value"]
    return out


def _headers() -> dict:
    # curl_cffi's impersonate sets UA + sec-ch-* + TLS fingerprint to match
    # Chrome; we only add what the app expects on top.
    return {
        "Accept": "text/event-stream, application/json",
        "Accept-Language": "en-US,en;q=0.9",
        "Content-Type": "application/json",
        "Origin": _BASE,
        "Referer": f"{_BASE}/chats",
    }


async def _org_id(s: AsyncSession, cookies: dict) -> str:
    r = await s.get(f"{_BASE}/api/organizations", headers=_headers(), cookies=cookies, impersonate=_IMPERSONATE)
    if r.status_code >= 400:
        raise RuntimeError(f"claude /organizations {r.status_code}: {r.text[:300]}")
    orgs = r.json()
    if not orgs:
        raise RuntimeError("no claude organizations found on this account")
    for o in orgs:
        caps = o.get("capabilities") or []
        if "chat" in caps:
            return o["uuid"]
    return orgs[0]["uuid"]


def _flatten(messages: list[dict]) -> tuple[str, str]:
    sys_parts = [m["content"] for m in messages if m.get("role") == "system" and isinstance(m.get("content"), str)]
    convo = []
    for m in messages:
        role = m.get("role")
        if role == "system":
            continue
        content = m.get("content")
        if isinstance(content, list):
            content = "".join(p.get("text", "") for p in content if isinstance(p, dict) and p.get("type") == "text")
        if not isinstance(content, str):
            content = str(content)
        tag = {"user": "Human", "assistant": "Assistant"}.get(role, role.title())
        convo.append(f"{tag}: {content}")
    prompt = "\n\n".join(convo)
    if not prompt.endswith("Assistant:"):
        prompt += "\n\nAssistant:"
    return ("\n\n".join(sys_parts), prompt)


def _base_payload(prompt: str, system: str) -> dict:
    payload = {
        "prompt": prompt,
        "parent_message_uuid": "00000000-0000-4000-8000-000000000000",
        "timezone": "America/Los_Angeles",
        "attachments": [],
        "files": [],
        "sync_sources": [],
        "rendering_mode": "messages",
    }
    if system:
        payload["personalized_styles"] = [{"key": "custom", "instructions": system}]
    return payload


async def _attempt(s: AsyncSession, org: str, cookies: dict, payload: dict) -> AsyncIterator[dict]:
    conv_id = str(uuid.uuid4())
    r = await s.post(
        f"{_BASE}/api/organizations/{org}/chat_conversations",
        headers=_headers(),
        cookies=cookies,
        json={"uuid": conv_id, "name": ""},
        impersonate=_IMPERSONATE,
    )
    if r.status_code >= 400:
        raise RuntimeError(f"claude conversation create {r.status_code}: {r.text[:300]}")

    url = f"{_BASE}/api/organizations/{org}/chat_conversations/{conv_id}/completion"
    try:
        async with s.stream(
            "POST", url, headers=_headers(), cookies=cookies, json=payload, impersonate=_IMPERSONATE
        ) as resp:
            if resp.status_code >= 400:
                body = await resp.atext()
                if "model_not_available" in body:
                    raise _ModelNotAvailable()
                raise RuntimeError(f"claude error {resp.status_code}: {body[:400]}")
            async for line in resp.aiter_lines():
                if isinstance(line, bytes):
                    line = line.decode("utf-8", "ignore")
                line = line.strip()
                if not line or not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                if not data or data == "[DONE]":
                    continue
                try:
                    evt = json.loads(data)
                except json.JSONDecodeError:
                    continue
                etype = evt.get("type")
                if etype == "completion":
                    chunk = evt.get("completion", "")
                    if chunk:
                        yield {"delta": chunk}
                elif etype == "content_block_delta":
                    d = evt.get("delta") or {}
                    if d.get("type") == "text_delta":
                        yield {"delta": d.get("text", "")}
                elif etype == "message_stop":
                    break
    finally:
        try:
            await s.request(
                "DELETE",
                f"{_BASE}/api/organizations/{org}/chat_conversations/{conv_id}",
                headers=_headers(),
                cookies=cookies,
                impersonate=_IMPERSONATE,
            )
        except Exception:
            pass


async def stream(req: dict) -> AsyncIterator[dict]:
    session = store.load_session("claude")
    if not session:
        raise RuntimeError("claude not connected — open the dashboard and click Connect Claude")

    model = req.get("model", "claude-sonnet-4-5")
    system, prompt = _flatten(req.get("messages", []))
    cookies = _cookies(session)

    async with AsyncSession(impersonate=_IMPERSONATE, timeout=300) as s:
        org = await _org_id(s, cookies)

        payload = _base_payload(prompt, system)
        web_model = _WEB_MODEL.get(model)
        if web_model:
            payload["model"] = web_model

        if "model" in payload:
            try:
                async for d in _attempt(s, org, cookies, payload):
                    yield d
                return
            except _ModelNotAvailable:
                pass  # fall through: retry with account default (no model field)

        default_payload = _base_payload(prompt, system)
        async for d in _attempt(s, org, cookies, default_payload):
            yield d


def openai_chunk(model: str, delta_text: str, finish: str | None = None) -> dict:
    return {
        "id": f"chatcmpl-{uuid.uuid4().hex[:24]}",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {"content": delta_text} if delta_text else {},
            "finish_reason": finish,
        }],
    }
