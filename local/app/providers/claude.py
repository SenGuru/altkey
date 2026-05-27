import json
import time
import uuid
from typing import AsyncIterator

import httpx

from .. import store
from ..harvester import cookie_header

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


def _headers(session: dict) -> dict:
    return {
        "User-Agent": session.get("user_agent") or "Mozilla/5.0",
        "Accept": "text/event-stream, application/json",
        "Accept-Language": "en-US,en;q=0.9",
        "Content-Type": "application/json",
        "Origin": _BASE,
        "Referer": f"{_BASE}/chats",
        "Cookie": cookie_header(session, _DOMAINS),
    }


async def _org_id(client: httpx.AsyncClient, session: dict) -> str:
    r = await client.get(f"{_BASE}/api/organizations", headers=_headers(session))
    r.raise_for_status()
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


async def stream(req: dict) -> AsyncIterator[dict]:
    session = store.load_session("claude")
    if not session:
        raise RuntimeError("claude not connected — open the dashboard and click Connect Claude")

    model = req.get("model", "claude-sonnet-4-5")
    system, prompt = _flatten(req.get("messages", []))

    async with httpx.AsyncClient(http2=True, timeout=httpx.Timeout(120.0, read=300.0)) as client:
        org = await _org_id(client, session)

        conv_id = str(uuid.uuid4())
        r = await client.post(
            f"{_BASE}/api/organizations/{org}/chat_conversations",
            headers=_headers(session),
            json={"uuid": conv_id, "name": ""},
        )
        r.raise_for_status()

        payload = {
            "prompt": prompt,
            "parent_message_uuid": "00000000-0000-4000-8000-000000000000",
            "timezone": "America/Los_Angeles",
            "attachments": [],
            "files": [],
            "sync_sources": [],
            "rendering_mode": "messages",
            "model": model,
        }
        if system:
            payload["personalized_styles"] = [{"key": "custom", "instructions": system}]

        url = f"{_BASE}/api/organizations/{org}/chat_conversations/{conv_id}/completion"
        try:
            async with client.stream("POST", url, headers=_headers(session), json=payload) as resp:
                if resp.status_code >= 400:
                    body = await resp.aread()
                    raise RuntimeError(f"claude error {resp.status_code}: {body[:400]!r}")
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
                await client.delete(
                    f"{_BASE}/api/organizations/{org}/chat_conversations/{conv_id}",
                    headers=_headers(session),
                )
            except Exception:
                pass


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
