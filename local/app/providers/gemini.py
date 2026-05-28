import json
import re
import uuid
from typing import AsyncIterator

from curl_cffi import CurlMime
from curl_cffi.requests import AsyncSession

from .. import store

_PUSH_URL = "https://content-push.googleapis.com/upload/"
_PUSH_ID = "feeds/mcudyrk2a4khkz"

NAME = "gemini"
# All names are accepted and never error; under the hood they currently route
# to the account's default Gemini model (fine-grained selection via the
# x-goog-ext header is not yet wired — see stream()).
MODELS = [
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.0-flash",
    "gemini-2.0-flash-thinking",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
    "gemini-pro",
]

_BASE = "https://gemini.google.com"
_DOMAINS = ("google.com",)
_IMPERSONATE = "chrome"
_SNLM_RE = re.compile(r'"SNlM0e":"([^"]+)"')

_MODEL_HEADER = {
    "gemini-2.5-pro": ["c_a065d44e", 1],
    "gemini-2.5-flash": ["c_a065d44e", 0],
    "gemini-2.0-flash": ["c_835d8b8c", 0],
    "gemini-1.5-pro": ["c_70f59a40", 1],
}


async def detect(session: dict) -> list[str]:
    """Gemini has no clean model-list endpoint; cache the static catalog so
    /v1/models is consistent with the other providers."""
    store.save_detected_models("gemini", MODELS)
    return MODELS


def _cookies(session: dict) -> dict:
    out = {}
    for c in session.get("cookies", []):
        dom = (c.get("domain") or "").lstrip(".")
        if any(dom.endswith(d) for d in _DOMAINS):
            out[c["name"]] = c["value"]
    return out


def _headers(content_type: str | None = None) -> dict:
    h = {
        "Accept": "*/*",
        "Accept-Language": "en-US,en;q=0.9",
        "Origin": _BASE,
        "Referer": f"{_BASE}/app",
        "X-Same-Domain": "1",
    }
    if content_type:
        h["Content-Type"] = content_type
    return h


async def _snlm0e(s: AsyncSession, cookies: dict) -> str:
    r = await s.get(f"{_BASE}/app", headers=_headers(), cookies=cookies, impersonate=_IMPERSONATE)
    if r.status_code != 200:
        raise RuntimeError(f"gemini /app {r.status_code} — re-connect in dashboard")
    m = _SNLM_RE.search(r.text)
    if not m:
        raise RuntimeError("gemini session expired — re-connect in dashboard")
    return m.group(1)


async def _upload_image(s: AsyncSession, data: bytes, mime: str) -> str:
    """Upload to Google's content-push endpoint. Returns the file identifier
    string used inside f.req. No cookies needed for this bucket."""
    ext = {"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif"}.get(mime, "png")
    filename = f"image-{uuid.uuid4().hex[:8]}.{ext}"
    mp = CurlMime()
    mp.addpart(name="file", content_type=mime, filename=filename, data=data)
    try:
        r = await s.post(
            _PUSH_URL,
            headers={"Push-ID": _PUSH_ID},
            multipart=mp,
            impersonate=_IMPERSONATE,
        )
    finally:
        mp.close()
    if r.status_code >= 400:
        raise RuntimeError(f"gemini image upload {r.status_code}: {r.text[:200]}")
    return r.text.strip()


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

    from ._imgutil import extract_images

    model = req.get("model", "gemini-2.5-flash")
    messages = req.get("messages", [])
    prompt = _flatten(messages)
    cookies = _cookies(session)
    images = extract_images(messages)

    async with AsyncSession(impersonate=_IMPERSONATE, timeout=300) as s:
        snlm = await _snlm0e(s, cookies)
        req_id = str(int(uuid.uuid4().int % 1_000_000))

        uploaded = []
        for data, mime in images:
            ext = {"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif"}.get(mime, "png")
            fid = await _upload_image(s, data, mime)
            uploaded.append([[fid], f"image-{uuid.uuid4().hex[:8]}.{ext}"])

        # f.req for a fresh conversation. With images, the prompt slot becomes
        # [prompt, 0, None, [[[fid], filename], ...]]; otherwise just [prompt].
        if uploaded:
            inner = [[prompt, 0, None, uploaded], None, None]
        else:
            inner = [[prompt], None, None]
        f_req = json.dumps([None, json.dumps(inner)])
        form = {"f.req": f_req, "at": snlm}
        url = f"{_BASE}/_/BardChatUi/data/assistant.lamda.BardFrontendService/StreamGenerate"
        params = {"bl": "boq_assistant-bard-web-server", "_reqid": req_id, "rt": "c"}

        headers = _headers("application/x-www-form-urlencoded;charset=UTF-8")
        # Model selection via x-goog-ext header is unverified/fragile; for now we
        # use the account's default Gemini model. (See _MODEL_HEADER TODO.)
        model_hdr = None
        if model_hdr:
            headers["x-goog-ext-525001261-jspb"] = json.dumps(model_hdr)

        last_text = ""
        async with s.stream(
            "POST",
            url,
            headers=headers,
            cookies=cookies,
            params=params,
            data=form,
            impersonate=_IMPERSONATE,
        ) as resp:
            if resp.status_code >= 400:
                body = await resp.atext()
                raise RuntimeError(f"gemini error {resp.status_code}: {body[:400]}")
            buf = ""
            async for chunk in resp.aiter_content():
                if isinstance(chunk, bytes):
                    chunk = chunk.decode("utf-8", "ignore")
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
