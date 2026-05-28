import json
import uuid
from typing import AsyncIterator

from curl_cffi.requests import AsyncSession

from .. import store

NAME = "chatgpt"
MODELS = [
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-1",
    "gpt-4-1-mini",
    "o1",
    "o3",
    "o4-mini",
    "chatgpt-4o-latest",
]

_BASE = "https://chatgpt.com"
_DOMAINS = ("chatgpt.com", "chat.openai.com", "openai.com")
_IMPERSONATE = "chrome"


def _cookies(session: dict) -> dict:
    out = {}
    for c in session.get("cookies", []):
        dom = (c.get("domain") or "").lstrip(".")
        if any(dom.endswith(d) for d in _DOMAINS):
            out[c["name"]] = c["value"]
    return out


def _headers(access_token: str | None = None, oai_device: str | None = None) -> dict:
    h = {
        "Accept": "text/event-stream",
        "Accept-Language": "en-US,en;q=0.9",
        "Origin": _BASE,
        "Referer": f"{_BASE}/",
    }
    if access_token:
        h["Authorization"] = f"Bearer {access_token}"
        h["Content-Type"] = "application/json"
    if oai_device:
        h["OAI-Device-Id"] = oai_device
    return h


async def _access_token(s: AsyncSession, cookies: dict) -> str:
    r = await s.get(f"{_BASE}/api/auth/session", headers=_headers(), cookies=cookies, impersonate=_IMPERSONATE)
    if r.status_code != 200:
        raise RuntimeError(f"chatgpt auth/session {r.status_code} — re-connect in dashboard")
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
            content = "".join(p.get("text", "") for p in content if isinstance(p, dict) and p.get("type") == "text")
        if not isinstance(content, str):
            content = str(content)
        out.append({
            "id": str(uuid.uuid4()),
            "author": {"role": "system" if role == "system" else ("assistant" if role == "assistant" else "user")},
            "content": {"content_type": "text", "parts": [content]},
            "metadata": {},
        })
    return out


def _device_id(session: dict) -> str:
    for c in session.get("cookies", []):
        if c.get("name") == "oai-did":
            return c.get("value", "")
    return str(uuid.uuid4())


async def stream(req: dict) -> AsyncIterator[dict]:
    session = store.load_session("chatgpt")
    if not session:
        raise RuntimeError("chatgpt not connected — open the dashboard and click Connect ChatGPT")

    model = req.get("model", "gpt-4o")
    parts = _to_parts(req.get("messages", []))
    cookies = _cookies(session)
    oai_device = _device_id(session)

    async with AsyncSession(impersonate=_IMPERSONATE, timeout=300) as s:
        token = await _access_token(s, cookies)

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
            "suggestions": [],
        }

        url = f"{_BASE}/backend-api/conversation"
        last_text = ""
        async with s.stream(
            "POST", url, headers=_headers(token, oai_device), cookies=cookies, json=payload, impersonate=_IMPERSONATE
        ) as resp:
            if resp.status_code >= 400:
                body = await resp.atext()
                raise RuntimeError(f"chatgpt error {resp.status_code}: {body[:400]}")
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
                if evt.get("type") == "moderation":
                    continue
                msg = evt.get("message")
                if not msg:
                    continue
                if msg.get("author", {}).get("role") != "assistant":
                    continue
                content = msg.get("content") or {}
                parts_list = content.get("parts") or []
                if not parts_list or not isinstance(parts_list[0], str):
                    continue
                text = parts_list[0]
                if text.startswith(last_text):
                    delta = text[len(last_text):]
                    last_text = text
                    if delta:
                        yield {"delta": delta}
                else:
                    last_text = text
                    yield {"delta": text}
