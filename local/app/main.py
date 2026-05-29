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

store.init()

# When ALTKEY_ADMIN_TOKEN is set (any non-local/hosted deployment), every
# /admin/* call must present it as `X-Admin-Token`. Unset = local mode, open
# (the server is bound to 127.0.0.1).
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

# CORS for the local dashboard. Server is bound to 127.0.0.1 only.
app.add_middleware(
    CORSMiddleware,
    allow_origin_regex=r"^https?://(127\.0\.0\.1|localhost)(:\d+)?$",
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


@app.get("/callback", response_class=HTMLResponse)
async def oauth_callback(code: str = "", state: str = "", error: str = ""):
    """Claude redirects here after approval — we exchange the code automatically.
    No pasting needed."""
    from .providers import claude_oauth
    if error:
        return f"<h2>Connect failed</h2><p>{error}</p><p>Close this tab and try again.</p>"
    if not code:
        return "<h2>Missing code</h2><p>Close this tab and try again.</p>"
    try:
        res = await claude_oauth.finish_oauth(f"{code}#{state}" if state else code)
        asyncio.create_task(providers.detect_one("claude_oauth"))
        sub = res.get("subscription", "?")
        return (f"<h2>✅ Claude connected</h2><p>Subscription: <b>{sub}</b>. Tool calling enabled.</p>"
                "<p>You can close this tab and return to altkey.</p>"
                "<script>setTimeout(()=>window.close(),2500)</script>")
    except Exception as e:
        return f"<h2>Connect failed</h2><pre>{str(e)[:400]}</pre><p>Close this tab and try again.</p>"


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

    # All live providers are NATIVE (OAuth → real API), handling the full
    # OpenAI surface (tool calls, vision) and emitting OpenAI-formatted output.
    if not getattr(mod, "NATIVE", False):
        raise HTTPException(500, f"provider {mod.NAME} is not wired for native OpenAI output")
    if want_stream:
        return StreamingResponse(mod.openai_stream(body), media_type="text/event-stream")
    try:
        return await mod.openai_completion(body)
    except Exception as e:
        raise HTTPException(502, f"upstream error: {e}")


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


@app.post("/v1/responses")
async def v1_responses(req: Request):
    """OpenAI Responses API proxy — routes through ChatGPT Codex OAuth.
    Tools that hit api.openai.com/v1/responses (gstack design-shotgun,
    OpenAI Agents SDK, etc.) work by setting OPENAI_BASE_URL to altkey."""
    _auth(req)
    body = await req.json()
    from .providers import chatgpt
    try:
        return await chatgpt.proxy_responses(body)
    except Exception as e:
        raise HTTPException(502, f"upstream error: {e}")


@app.post("/v1/images/generations")
async def v1_images(req: Request):
    """OpenAI-compatible image generation. Routes to ChatGPT's image_generation
    tool (Gemini support can be added later). Returns b64_json."""
    _auth(req)
    body = await req.json()
    prompt = body.get("prompt", "")
    if not prompt:
        raise HTTPException(400, "missing prompt")
    n = int(body.get("n", 1))
    from .providers import chatgpt
    try:
        images = await chatgpt.generate_image(prompt, n)
    except Exception as e:
        raise HTTPException(502, f"image generation error: {e}")
    if not images:
        raise HTTPException(502, "no image returned")
    return {"created": int(time.time()), "data": [{"b64_json": b} for b in images]}


class ConnectReq(BaseModel):
    provider: str


@app.get("/admin/status", dependencies=[Depends(_check_admin)])
async def admin_status():
    return {
        "sessions": store.list_sessions(),
        "keys": store.list_keys(),
    }


@app.post("/admin/disconnect", dependencies=[Depends(_check_admin)])
async def admin_disconnect(body: ConnectReq):
    store.delete_session(body.provider)
    return {"ok": True}


class MintReq(BaseModel):
    label: str = ""
    provider: str | None = None  # None = all providers; else claude/chatgpt (gemini parked)


@app.post("/admin/connect-cli", dependencies=[Depends(_check_admin)])
async def admin_connect_cli(body: ConnectReq):
    """Connect a provider by reading its CLI's OAuth credentials from disk.
    - claude  → ~/.claude/.credentials.json (Claude Code login)
    - chatgpt → ~/.codex/auth.json (OpenAI Codex CLI login)
    Both are real-API OAuth tokens; no cookies, no impersonation."""
    if body.provider == "claude":
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

    if body.provider == "chatgpt":
        path = Path.home() / ".codex" / "auth.json"
        if not path.exists():
            return JSONResponse({"ok": False, "error": "no Codex CLI credentials found — run `codex` and log in first"}, status_code=400)
        try:
            d = json.loads(path.read_text())
            tokens = d.get("tokens") or {}
            if not tokens.get("access_token"):
                raise ValueError("auth.json has no tokens.access_token")
        except Exception as e:
            return JSONResponse({"ok": False, "error": f"could not read credentials: {e}"}, status_code=400)
        store.save_session("chatgpt", {
            "source": "codex_cli",
            "account_id": tokens.get("account_id", ""),
        })
        asyncio.create_task(providers.detect_one("chatgpt"))
        return {"ok": True, "provider": "chatgpt", "mode": "oauth", "account_id": tokens.get("account_id", "")}

    return JSONResponse({"ok": False, "error": f"unsupported provider: {body.provider}"}, status_code=400)


@app.post("/admin/oauth/start", dependencies=[Depends(_check_admin)])
async def admin_oauth_start():
    from .providers import claude_oauth
    return claude_oauth.start_oauth()


class OAuthFinishReq(BaseModel):
    code: str


@app.post("/admin/oauth/finish", dependencies=[Depends(_check_admin)])
async def admin_oauth_finish(body: OAuthFinishReq):
    from .providers import claude_oauth
    try:
        res = await claude_oauth.finish_oauth(body.code)
    except Exception as e:
        return JSONResponse({"ok": False, "error": str(e)}, status_code=400)
    asyncio.create_task(providers.detect_one("claude_oauth"))
    return res


@app.post("/admin/detect", dependencies=[Depends(_check_admin)])
async def admin_detect():
    return {"ok": True, "detected": await providers.detect_all()}


@app.post("/admin/keys", dependencies=[Depends(_check_admin)])
async def admin_mint(body: MintReq):
    prov = body.provider
    if prov not in (None, "claude", "chatgpt"):  # "gemini" parked
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
