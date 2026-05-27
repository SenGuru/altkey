import json
import re
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

_MODEL_HEADER = {
    "gemini-2.5-pro": ["c_a065d44e", 1],
    "gemini-2.5-flash": ["c_a065d44e", 0],
    "gemini-2.0-flash": ["c_835d8b8c", 0],
    "gemini-1.5-pro": ["c_70f59a40", 1],
}


def _headers(session: dict, content_type: str | None = None) -> dict:
    h = {
        "User-Agent": session.get("user_agent") or "Mozilla/5.0",
        "Accept": "*/*",
        "Accept-Language": "en-US,en;q=0.9",
        "Origin": _BASE,
        "Referer": f"{_BASE}/app",
        "X-Same-Domain": "1",
        "Cookie": cookie_header(session, _DOMAINS),
    }
    if content_type:
        h["Content-Type"] = content_type
    return h


async def _snlm0e(client: httpx.AsyncClient, session: dict) -> str:
    r = await client.get(f"{_BASE}/app", headers=_headers(session))
    if r.status_code != 200:
        raise RuntimeError(f"gemini /app {r.status_code} — re-connect in dashboard")
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
            content = "".join(p.get("text", "") for p in content if isinstance(p, dict) and p.get("type") == "text")
        if not isinstance(content, str):
            content = str(content)
        tag = {"user": "User", "assistant": "Assistant", "system": "System"}.get(role, role.title())
        parts.append(f"{tag}: {content}")
    return "\n\n".join(parts)


async def stream(req: dict) -> AsyncIterator[dict]:
    session = store.load_session("gemini")
    if not session:
        raise RuntimeError("gemini not connected — open the dashboard and click Connect Gemini")

    model = req.get("model", "gemini-2.5-flash")
    prompt = _flatten(req.get("messages", []))

    async with httpx.AsyncClient(http2=True, timeout=httpx.Timeout(180.0, read=300.0), follow_redirects=True) as client:
        snlm = await _snlm0e(client, session)
        model_hdr = _MODEL_HEADER.get(model, _MODEL_HEADER["gemini-2.5-flash"])
        req_id = str(int(uuid.uuid4().int % 1_000_000))

        inner = [[prompt], None, [None, None, None, [], None, None, "", 0, 0, 0, [], 0, 0, None, 0, 0, [], 0, 0, model_hdr]]
        f_req = json.dumps([None, json.dumps(inner)])
        form = {"f.req": f_req, "at": snlm}
        url = f"{_BASE}/_/BardChatUi/data/assistant.lamda.BardFrontendService/StreamGenerate"
        params = {"bl": "boq_assistant-bard-web-server", "_reqid": req_id, "rt": "c"}

        last_text = ""
        async with client.stream(
            "POST",
            url,
            headers=_headers(session, "application/x-www-form-urlencoded;charset=UTF-8"),
            params=params,
            data=form,
        ) as resp:
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
                    if not line or line.startswith(")]}'") or line.isdigit():
                        continue
                    try:
                        outer = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if not isinstance(outer, list):
                        continue
                    for row in outer:
                        if not isinstance(row, list) or len(row) < 3:
                            continue
                        inner_payload = row[2]
                        if not isinstance(inner_payload, str):
                            continue
                        try:
                            data = json.loads(inner_payload)
                        except json.JSONDecodeError:
                            continue
                        try:
                            cands = data[4] if len(data) > 4 else None
                            if not cands or not isinstance(cands, list):
                                continue
                            first = cands[0]
                            if not first or len(first) < 2 or not first[1]:
                                continue
                            text = first[1][0] if isinstance(first[1], list) and first[1] else ""
                            if not isinstance(text, str) or not text:
                                continue
                            if text.startswith(last_text):
                                delta = text[len(last_text):]
                                last_text = text
                                if delta:
                                    yield {"delta": delta}
                            else:
                                last_text = text
                                yield {"delta": text}
                        except (IndexError, TypeError):
                            continue
