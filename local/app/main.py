import asyncio
import json
import os
import time
import uuid
from pathlib import Path

from fastapi import Depends, FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse, JSONResponse, StreamingResponse
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel

from . import providers, store
from .harvester import harvest
from .providers.claude import openai_chunk

store.init()

# When ALTKEY_ADMIN_TOKEN is set (any non-local/hosted deployment), every
# /admin/* call must present it as `X-Admin-Token`. Unset = local mode, open
# (the server is bound to 127.0.0.1 and the companion extension talks to it).
_ADMIN_TOKEN = os.environ.get("ALTKEY_ADMIN_TOKEN", "")


def _check_admin(request: Request) -> None:
    if not _ADMIN_TOKEN:
        return
    if request.headers.get("x-admin-token") != _ADMIN_TOKEN:
        raise HTTPException(403, "admin token required")

app = FastAPI(title="altkey", docs_url=None, redoc_url=None)


@app.on_event("startup")
async def _startup():
    # Server deploys: seed the Claude OAuth token from a secret (no ~/.claude
    # file on a headless host) and keep it refreshed proactively.
    from .providers import claude_oauth
    seeded = claude_oauth.seed_from_env()
    if seeded:
        print("[altkey] seeded claude_oauth token from ALTKEY_CLAUDE_OAUTH_JSON")
    asyncio.create_task(claude_oauth.refresh_loop())

# Allow the companion browser extension (chrome-extension:// / moz-extension://)
# to call the admin capture endpoint. Server is bound to 127.0.0.1 only.
app.add_middleware(
    CORSMiddleware,
    allow_origin_regex=r"^(chrome-extension|moz-extension)://.*$",
    allow_methods=["*"],
    allow_headers=["*"],
)

DASHBOARD = (Path(__file__).parent / "dashboard.html").read_text(encoding="utf-8")


# Transparent mode: when altkey intercepts hardcoded endpoints (api.openai.com
# etc), the calling app sends whatever key it was given — possibly a real
# OpenAI key or a dummy. So in transparent mode we accept any key.
_TRANSPARENT = os.environ.get("ALTKEY_TRANSPARENT") == "1"


def _auth(req: Request) -> str:
    """Validate the key and return it (so callers can resolve its provider scope)."""
    # Accept OpenAI-style (Authorization: Bearer) or Anthropic-style (x-api-key).
    key = ""
    hdr = req.headers.get("authorization", "")
    if hdr.lower().startswith("bearer "):
        key = hdr.split(" ", 1)[1].strip()
    elif req.headers.get("x-api-key"):
        key = req.headers["x-api-key"].strip()
    if _TRANSPARENT:
        return key  # any key (or none) accepted; no scope enforced
    if not key:
        raise HTTPException(401, "missing api key")
    if not store.key_exists(key):
        raise HTTPException(401, "invalid api key")
    return key


@app.get("/", response_class=HTMLResponse)
async def dashboard() -> str:
    return DASHBOARD


@app.get("/v1/models")
async def v1_models(req: Request) -> dict:
    key = _auth(req)
    scope = store.key_provider(key)
    models = providers.list_models()
    if scope:
        models = [m for m in models if m["owned_by"] == scope]
    return {"object": "list", "data": models}


@app.post("/v1/chat/completions")
async def v1_chat(req: Request):
    key = _auth(req)
    body = await req.json()
    model = body.get("model") or "claude-sonnet-4-5"
    mod = providers.for_model(model)
    if mod is None:
        raise HTTPException(400, f"unknown model: {model}")
    scope = store.key_provider(key)
    if scope and mod.NAME != scope:
        raise HTTPException(403, f"this key is scoped to '{scope}' and cannot use model '{model}' ({mod.NAME})")
    want_stream = bool(body.get("stream"))

    # NATIVE providers (OAuth → real API) handle the full OpenAI surface
    # themselves, including tool calls, and emit OpenAI-formatted output.
    if getattr(mod, "NATIVE", False):
        if want_stream:
            return StreamingResponse(mod.openai_stream(body), media_type="text/event-stream")
        try:
            return await mod.openai_completion(body)
        except Exception as e:
            raise HTTPException(502, f"upstream error: {e}")

    if want_stream:
        async def gen():
            try:
                async for evt in mod.stream(body):
                    yield "data: " + json.dumps(openai_chunk(model, evt.get("delta", ""))) + "\n\n"
                yield "data: " + json.dumps(openai_chunk(model, "", finish="stop")) + "\n\n"
                yield "data: [DONE]\n\n"
            except Exception as e:
                err = {"error": {"message": str(e), "type": "upstream_error"}}
                yield "data: " + json.dumps(err) + "\n\n"
                yield "data: [DONE]\n\n"
        return StreamingResponse(gen(), media_type="text/event-stream")

    text_parts: list[str] = []
    try:
        async for evt in mod.stream(body):
            text_parts.append(evt.get("delta", ""))
    except Exception as e:
        raise HTTPException(502, f"upstream error: {e}")
    content = "".join(text_parts)
    return {
        "id": f"chatcmpl-{uuid.uuid4().hex[:24]}",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop",
        }],
        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
    }


@app.post("/v1/messages")
async def v1_messages(req: Request):
    """Native Anthropic Messages API — lets Anthropic-SDK clients (e.g. Hermes'
    'anthropic' provider) point ANTHROPIC_BASE_URL at altkey. Backed by the
    Claude OAuth provider."""
    key = _auth(req)
    scope = store.key_provider(key)
    if scope and scope != "claude":
        raise HTTPException(403, f"this key is scoped to '{scope}', not claude")
    from .providers import claude_oauth
    if not store.load_session("claude_oauth"):
        raise HTTPException(400, "claude (oauth) not connected — run Connect Claude (CLI)")
    body = await req.json()
    if body.get("stream"):
        return StreamingResponse(claude_oauth.anthropic_messages_stream(body), media_type="text/event-stream")
    status, data = await claude_oauth.anthropic_messages(body)
    return JSONResponse(data, status_code=status)


class ConnectReq(BaseModel):
    provider: str


@app.get("/admin/status", dependencies=[Depends(_check_admin)])
async def admin_status():
    return {
        "sessions": store.list_sessions(),
        "keys": store.list_keys(),
    }


@app.post("/admin/connect", dependencies=[Depends(_check_admin)])
async def admin_connect(body: ConnectReq):
    try:
        res = await harvest(body.provider)
    except Exception as e:
        return JSONResponse({"ok": False, "error": str(e)}, status_code=400)
    return {"ok": True, **res}


@app.post("/admin/disconnect", dependencies=[Depends(_check_admin)])
async def admin_disconnect(body: ConnectReq):
    store.delete_session(body.provider)
    return {"ok": True}


_IMPORT_DOMAIN = {
    "claude": ".claude.ai",
    "chatgpt": ".chatgpt.com",
    "gemini": ".google.com",
}
_IMPORT_REQUIRED = {
    "claude": ["sessionKey"],
    "chatgpt": ["__Secure-next-auth.session-token"],
    "gemini": ["__Secure-1PSID"],
}
_DEFAULT_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"
)


class ImportReq(BaseModel):
    provider: str
    cookies: str
    user_agent: str = ""


def _parse_cookie_string(s: str) -> list[tuple[str, str]]:
    out = []
    for part in s.replace("\n", ";").split(";"):
        part = part.strip()
        if not part or "=" not in part:
            continue
        name, _, value = part.partition("=")
        name = name.strip()
        value = value.strip()
        if name:
            out.append((name, value))
    return out


class CaptureCookie(BaseModel):
    name: str
    value: str
    domain: str = ""
    path: str = "/"
    secure: bool = True


class CaptureReq(BaseModel):
    provider: str
    cookies: list[CaptureCookie]
    user_agent: str = ""


@app.post("/admin/capture", dependencies=[Depends(_check_admin)])
async def admin_capture(body: CaptureReq):
    """Companion-extension entrypoint. Receives structured cookies read via the
    browser's chrome.cookies API after an explicit user 'Connect' click."""
    if body.provider not in _IMPORT_DOMAIN:
        return JSONResponse({"ok": False, "error": f"unknown provider: {body.provider}"}, status_code=400)
    names = [c.name for c in body.cookies]
    missing = [
        p for p in _IMPORT_REQUIRED[body.provider]
        if not any(n.startswith(p) for n in names)
    ]
    if missing:
        return JSONResponse(
            {"ok": False, "error": f"not logged in — missing: {', '.join(missing)}"},
            status_code=400,
        )
    cookies = [
        {
            "name": c.name,
            "value": c.value,
            "domain": c.domain or _IMPORT_DOMAIN[body.provider],
            "path": c.path or "/",
            "secure": c.secure,
        }
        for c in body.cookies
    ]
    store.save_session(body.provider, {"cookies": cookies, "user_agent": body.user_agent or _DEFAULT_UA})
    asyncio.create_task(providers.detect_one(body.provider))
    return {"ok": True, "provider": body.provider, "cookie_count": len(cookies)}


@app.post("/admin/import", dependencies=[Depends(_check_admin)])
async def admin_import(body: ImportReq):
    if body.provider not in _IMPORT_DOMAIN:
        return JSONResponse({"ok": False, "error": f"unknown provider: {body.provider}"}, status_code=400)
    pairs = _parse_cookie_string(body.cookies)
    if not pairs:
        return JSONResponse({"ok": False, "error": "no cookies parsed — paste name=value pairs"}, status_code=400)

    names = [name for name, _ in pairs]
    missing = [
        c for c in _IMPORT_REQUIRED[body.provider]
        if not any(n.startswith(c) for n in names)
    ]
    if missing:
        return JSONResponse(
            {"ok": False, "error": f"missing required cookie(s): {', '.join(missing)}"},
            status_code=400,
        )

    domain = _IMPORT_DOMAIN[body.provider]
    cookies = [
        {"name": name, "value": value, "domain": domain, "path": "/", "secure": True}
        for name, value in pairs
    ]
    store.save_session(body.provider, {"cookies": cookies, "user_agent": body.user_agent or _DEFAULT_UA})
    asyncio.create_task(providers.detect_one(body.provider))
    return {"ok": True, "provider": body.provider, "cookie_count": len(cookies)}


class MintReq(BaseModel):
    label: str = ""
    provider: str | None = None  # None = all providers; else claude/chatgpt/gemini


@app.post("/admin/connect-cli", dependencies=[Depends(_check_admin)])
async def admin_connect_cli(body: ConnectReq):
    """Connect Claude via the local Claude Code OAuth token (real Anthropic API,
    enables tool calling). Reads ~/.claude/.credentials.json."""
    if body.provider != "claude":
        return JSONResponse({"ok": False, "error": "only claude CLI supported here"}, status_code=400)
    path = Path.home() / ".claude" / ".credentials.json"
    if not path.exists():
        return JSONResponse({"ok": False, "error": "no Claude Code credentials found — log into Claude Code first"}, status_code=400)
    try:
        oauth = json.loads(path.read_text())["claudeAiOauth"]
    except Exception as e:
        return JSONResponse({"ok": False, "error": f"could not read credentials: {e}"}, status_code=400)
    store.save_session("claude_oauth", {
        "accessToken": oauth["accessToken"],
        "refreshToken": oauth.get("refreshToken", ""),
        "expiresAt": oauth.get("expiresAt", 0),
        "subscriptionType": oauth.get("subscriptionType", "unknown"),
    })
    asyncio.create_task(providers.detect_one("claude_oauth"))
    return {"ok": True, "provider": "claude", "mode": "oauth", "subscription": oauth.get("subscriptionType")}


@app.post("/admin/detect", dependencies=[Depends(_check_admin)])
async def admin_detect():
    return {"ok": True, "detected": await providers.detect_all()}


@app.post("/admin/keys", dependencies=[Depends(_check_admin)])
async def admin_mint(body: MintReq):
    prov = body.provider
    if prov not in (None, "claude", "chatgpt", "gemini"):
        return JSONResponse({"ok": False, "error": f"invalid provider scope: {prov}"}, status_code=400)
    return {"key": store.mint_key(body.label, prov), "provider": prov}


class RevokeReq(BaseModel):
    key: str


@app.post("/admin/keys/revoke", dependencies=[Depends(_check_admin)])
async def admin_revoke(body: RevokeReq):
    store.revoke_key(body.key)
    return {"ok": True}


def run():
    import uvicorn
    host = os.environ.get("ALTKEY_HOST", "127.0.0.1")
    cert = os.environ.get("ALTKEY_TLS_CERT")
    key = os.environ.get("ALTKEY_TLS_KEY")
    if cert and key:
        # Transparent mode: serve HTTPS on 443 (or ALTKEY_PORT) for intercepted hosts.
        port = int(os.environ.get("ALTKEY_PORT", "443"))
        uvicorn.run("app.main:app", host=host, port=port, reload=False,
                    ssl_certfile=cert, ssl_keyfile=key)
    else:
        port = int(os.environ.get("ALTKEY_PORT", os.environ.get("PORT", "8787")))
        uvicorn.run("app.main:app", host=host, port=port, reload=False)


if __name__ == "__main__":
    run()
