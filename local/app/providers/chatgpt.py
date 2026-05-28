import asyncio
import base64
import binascii
import hashlib
import json
import random
import struct
import time
import uuid
from typing import AsyncIterator

from curl_cffi import CurlHttpVersion
from curl_cffi.requests import AsyncSession

from .. import store

NAME = "chatgpt"

# Models exposed via /v1/models. Real current web slugs + popular legacy/API
# aliases so tools that hardcode older names still work.
MODELS = [
    "gpt-5",
    "gpt-5-mini",
    "gpt-5-thinking",
    "gpt-5-instant",
    "o3",
    # legacy/API aliases (resolved to a real slug at request time):
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4.1",
    "gpt-4.1-mini",
    "o4-mini",
    "chatgpt-4o-latest",
]

# requested model name -> web backend slug. Anything not listed and not already
# a real slug falls back via heuristics in _resolve_model().
_ALIASES = {
    "gpt-5": "gpt-5-5",
    "gpt-5-thinking": "gpt-5-5-thinking",
    "gpt-5-instant": "gpt-5-5-instant",
    "gpt-5-mini": "gpt-5-mini",
    "gpt-4o": "gpt-5-5",
    "gpt-4o-latest": "gpt-5-5",
    "chatgpt-4o-latest": "gpt-5-5",
    "gpt-4.1": "gpt-5-5",
    "gpt-4-1": "gpt-5-5",
    "gpt-4.1-mini": "gpt-5-mini",
    "gpt-4o-mini": "gpt-5-mini",
    "o4-mini": "gpt-5-mini",
    "o1": "gpt-5-5-thinking",
    "o1-pro": "gpt-5-5-thinking",
    "o3": "o3",
    "o3-mini": "gpt-5-mini",
}


def _resolve_model(model: str) -> str:
    if not model:
        return "gpt-5-5"
    m = model.lower()
    # Already a real gpt-5 web slug → pass through unchanged.
    if m.startswith("gpt-5"):
        return model
    if m in _ALIASES:
        return _ALIASES[m]
    if m.startswith("o3"):
        return "o3"
    if "mini" in m:
        return "gpt-5-mini"
    if "think" in m or m.startswith("o1") or m.startswith("o4"):
        return "gpt-5-5-thinking"
    return "gpt-5-5"


_BASE = "https://chatgpt.com"
_DOMAINS = ("chatgpt.com", "chat.openai.com", "openai.com")
_IMPERSONATE = "chrome"


async def fetch_available_models(s: "AsyncSession", token: str, cookies: dict, device: str) -> list[str]:
    """Live list of model slugs this account can actually use."""
    r = await s.get(f"{_BASE}/backend-api/models", headers=_headers(token, device), cookies=cookies, impersonate=_IMPERSONATE)
    if r.status_code >= 400:
        return []
    data = r.json()
    return [m.get("slug") for m in (data.get("models") or []) if isinstance(m, dict) and m.get("slug")]


async def detect(session: dict) -> list[str]:
    """Cache the account's real available model slugs (via /backend-api/models)."""
    cookies = _cookies(session)
    device = _device_id(session)
    async with AsyncSession(impersonate=_IMPERSONATE, timeout=60) as s:
        token = await _access_token(s, cookies)
        slugs = await fetch_available_models(s, token, cookies, device)
    if slugs:
        store.save_detected_models("chatgpt", slugs)
    return slugs


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


def _msg_text(content) -> str:
    if isinstance(content, list):
        return "".join(p.get("text", "") for p in content if isinstance(p, dict) and p.get("type") == "text")
    return content if isinstance(content, str) else str(content)


def _to_parts(messages: list[dict], attachments: list[dict] | None = None) -> list[dict]:
    """Build ChatGPT message objects. If `attachments` is given (uploaded image
    metadata), the LAST user message becomes multimodal_text with asset
    pointers, and the files are recorded in metadata.attachments."""
    out = []
    last_user_idx = max((i for i, m in enumerate(messages) if m.get("role") == "user"), default=-1)
    for i, m in enumerate(messages):
        role = m.get("role", "user")
        author = "system" if role == "system" else ("assistant" if role == "assistant" else "user")
        text = _msg_text(m.get("content"))
        if attachments and i == last_user_idx:
            asset_parts = [
                {
                    "content_type": "image_asset_pointer",
                    "asset_pointer": f"file-service://{a['id']}",
                    "size_bytes": a["size"],
                    "width": a["width"],
                    "height": a["height"],
                }
                for a in attachments
            ]
            out.append({
                "id": str(uuid.uuid4()),
                "author": {"role": author},
                "content": {"content_type": "multimodal_text", "parts": [*asset_parts, text]},
                "metadata": {"attachments": [
                    {"id": a["id"], "name": a["name"], "size": a["size"],
                     "mimeType": a["mime"], "width": a["width"], "height": a["height"]}
                    for a in attachments
                ]},
            })
        else:
            out.append({
                "id": str(uuid.uuid4()),
                "author": {"role": author},
                "content": {"content_type": "text", "parts": [text]},
                "metadata": {},
            })
    return out


async def _upload_file(s: AsyncSession, token: str, cookies: dict, device: str, data: bytes, mime: str) -> dict:
    from ._imgutil import image_size

    w, h = image_size(data, mime)
    ext = {"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif"}.get(mime, "png")
    name = f"image-{uuid.uuid4().hex[:8]}.{ext}"
    h_json = _headers(token, device)
    h_json["Content-Type"] = "application/json"

    # 1) Register the file → get an Azure blob upload URL.
    r = await s.post(
        f"{_BASE}/backend-api/files",
        headers=h_json,
        cookies=cookies,
        json={"file_name": name, "file_size": len(data), "use_case": "multimodal",
              "timezone_offset_min": 420, "reset_rate_limits": False},
        impersonate=_IMPERSONATE,
    )
    if r.status_code >= 400:
        raise RuntimeError(f"chatgpt files register {r.status_code}: {r.text[:300]}")
    reg = r.json()
    file_id = reg["file_id"]
    upload_url = reg["upload_url"]

    # 2) PUT the bytes to the upload URL. The CDN is flaky; retry across
    # transports using a FRESH session each time (reusing the main impersonated
    # session for a different host triggers TLS errors).
    put_headers = {"x-ms-blob-type": "BlockBlob", "x-ms-version": "2020-04-08", "Content-Type": mime}
    last_err = None
    ok = False
    for attempt in range(4):
        try:
            if attempt % 2 == 0:
                async with AsyncSession(timeout=120) as bs:
                    p = await bs.put(upload_url, headers=put_headers, data=data, impersonate=_IMPERSONATE)
            else:
                async with AsyncSession(timeout=120, http_version=CurlHttpVersion.V1_1) as bs:
                    p = await bs.put(upload_url, headers=put_headers, data=data)
            if p.status_code < 400:
                ok = True
                break
            last_err = f"{p.status_code}: {p.text[:150]}"
        except Exception as e:
            last_err = str(e)[:150]
        await asyncio.sleep(0.4)
    if not ok:
        raise RuntimeError(f"chatgpt blob upload failed after retries: {last_err}")

    # 3) Mark the upload complete.
    done = await s.post(
        f"{_BASE}/backend-api/files/{file_id}/uploaded",
        headers=h_json,
        cookies=cookies,
        json={},
        impersonate=_IMPERSONATE,
    )
    if done.status_code >= 400:
        raise RuntimeError(f"chatgpt files finalize {done.status_code}: {done.text[:200]}")

    # 4) Poll until processed (image scanning).
    for _ in range(20):
        chk = await s.get(f"{_BASE}/backend-api/files/{file_id}", headers=h_json, cookies=cookies, impersonate=_IMPERSONATE)
        if chk.status_code < 400:
            st = (chk.json() or {}).get("retrieval_status") or (chk.json() or {}).get("status")
            if st in ("success", "processed", None):
                break
        await asyncio.sleep(0.5)

    return {"id": file_id, "name": name, "size": len(data), "mime": mime, "width": w, "height": h}


def _device_id(session: dict) -> str:
    for c in session.get("cookies", []):
        if c.get("name") == "oai-did":
            return c.get("value", "")
    return str(uuid.uuid4())


# --- ChatGPT sentinel proof-of-work -----------------------------------------
# chatgpt.com requires a chat-requirements token plus a solved proof-of-work
# on every conversation request, or it returns "Unusual activity detected".
# Algorithm reverse-engineered from the web client.

_POW_CORES = [8, 12, 16, 24]
_POW_SCREENS = [3000, 4000, 6000]


def _now_gmt() -> str:
    return time.strftime("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)", time.gmtime())


def _solve_pow(seed: str, difficulty: str, user_agent: str) -> str:
    core = random.choice(_POW_CORES)
    screen = random.choice(_POW_SCREENS)
    now = _now_gmt()
    config = [
        core + screen,
        now,
        4294705152,
        0,
        user_agent,
        "",
        "",
        "en-US",
        "en-US,en",
        0,
        "location",
        "scrollX",
        "_reactListeningg",
        now,
        random.random(),
    ]
    diff_len = len(difficulty)
    for i in range(300000):
        config[3] = i
        config[9] = round(1000 * (i / 300000))
        b = base64.b64encode(json.dumps(config).encode()).decode()
        h = hashlib.sha3_512((seed + b).encode()).hexdigest()
        if h[:diff_len] <= difficulty:
            return "gAAAAAB" + b
    # Fallback token (still better than nothing; server may accept degraded PoW).
    return "gAAAAABwQ8Lk5FbGpA2NcR9dShT6gYjU7VxZ4D" + base64.b64encode(seed.encode()).decode()


async def _chat_requirements(s: AsyncSession, token: str, cookies: dict, device: str, ua: str) -> dict:
    h = _headers(token, device)
    h["Content-Type"] = "application/json"
    r = await s.post(
        f"{_BASE}/backend-api/sentinel/chat-requirements",
        headers=h,
        cookies=cookies,
        json={"p": _solve_pow("0", "0", ua)},
        impersonate=_IMPERSONATE,
    )
    if r.status_code >= 400:
        raise RuntimeError(f"chatgpt chat-requirements {r.status_code}: {r.text[:300]}")
    return r.json()


async def stream(req: dict) -> AsyncIterator[dict]:
    session = store.load_session("chatgpt")
    if not session:
        raise RuntimeError("chatgpt not connected — open the dashboard and click Connect ChatGPT")

    from ._imgutil import extract_images

    model = _resolve_model(req.get("model", "gpt-5"))
    messages = req.get("messages", [])
    cookies = _cookies(session)
    oai_device = _device_id(session)
    images = extract_images(messages)

    ua = session.get("user_agent") or "Mozilla/5.0"

    async with AsyncSession(impersonate=_IMPERSONATE, timeout=300) as s:
        token = await _access_token(s, cookies)

        attachments = []
        for data, mime in images:
            attachments.append(await _upload_file(s, token, cookies, oai_device, data, mime))
        parts = _to_parts(messages, attachments or None)

        # Sentinel: fetch chat-requirements token + solve the proof-of-work.
        reqs = await _chat_requirements(s, token, cookies, oai_device, ua)
        req_token = reqs.get("token", "")
        pow_cfg = reqs.get("proofofwork") or {}
        proof_token = ""
        if pow_cfg.get("required"):
            proof_token = _solve_pow(pow_cfg.get("seed", ""), pow_cfg.get("difficulty", ""), ua)
        if (reqs.get("arkose") or {}).get("required"):
            raise RuntimeError(
                "chatgpt requires an Arkose challenge for this model/account — "
                "not solvable headlessly. Try gpt-4o-mini, or use the Claude provider."
            )

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

        h = _headers(token, oai_device)
        h["Content-Type"] = "application/json"
        if req_token:
            h["Openai-Sentinel-Chat-Requirements-Token"] = req_token
        if proof_token:
            h["Openai-Sentinel-Proof-Token"] = proof_token

        url = f"{_BASE}/backend-api/conversation"
        last_text = ""
        async with s.stream(
            "POST", url, headers=h, cookies=cookies, json=payload, impersonate=_IMPERSONATE
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
