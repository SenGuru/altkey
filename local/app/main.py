import asyncio
import json
import time
import uuid
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse, JSONResponse, StreamingResponse
from pydantic import BaseModel

from . import providers, store
from .harvester import harvest
from .providers.claude import openai_chunk

store.init()

app = FastAPI(title="altkey", docs_url=None, redoc_url=None)

DASHBOARD = (Path(__file__).parent / "dashboard.html").read_text(encoding="utf-8")


def _auth(req: Request) -> None:
    hdr = req.headers.get("authorization", "")
    if not hdr.lower().startswith("bearer "):
        raise HTTPException(401, "missing bearer token")
    key = hdr.split(" ", 1)[1].strip()
    if not store.key_exists(key):
        raise HTTPException(401, "invalid api key")


@app.get("/", response_class=HTMLResponse)
async def dashboard() -> str:
    return DASHBOARD


@app.get("/v1/models")
async def v1_models(req: Request) -> dict:
    _auth(req)
    return {"object": "list", "data": providers.list_models()}


@app.post("/v1/chat/completions")
async def v1_chat(req: Request):
    _auth(req)
    body = await req.json()
    model = body.get("model") or "claude-sonnet-4-5"
    mod = providers.for_model(model)
    if mod is None:
        raise HTTPException(400, f"unknown model: {model}")
    want_stream = bool(body.get("stream"))

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


class ConnectReq(BaseModel):
    provider: str


@app.get("/admin/status")
async def admin_status():
    return {
        "sessions": store.list_sessions(),
        "keys": store.list_keys(),
    }


@app.post("/admin/connect")
async def admin_connect(body: ConnectReq):
    try:
        res = await harvest(body.provider)
    except Exception as e:
        return JSONResponse({"ok": False, "error": str(e)}, status_code=400)
    return {"ok": True, **res}


@app.post("/admin/disconnect")
async def admin_disconnect(body: ConnectReq):
    store.delete_session(body.provider)
    return {"ok": True}


class MintReq(BaseModel):
    label: str = ""


@app.post("/admin/keys")
async def admin_mint(body: MintReq):
    return {"key": store.mint_key(body.label)}


class RevokeReq(BaseModel):
    key: str


@app.post("/admin/keys/revoke")
async def admin_revoke(body: RevokeReq):
    store.revoke_key(body.key)
    return {"ok": True}


def run():
    import uvicorn
    uvicorn.run("app.main:app", host="127.0.0.1", port=8787, reload=False)


if __name__ == "__main__":
    run()
