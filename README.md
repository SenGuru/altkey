# altkey

Local proxy that turns your AI subscription accounts (Claude Pro, ChatGPT Plus, Gemini Advanced) into a single OpenAI-compatible API key.

**This violates the ToS of every provider. Use only on your own accounts, only on localhost, never expose to the internet.**

## How it works

1. You run altkey locally on `127.0.0.1:8787`.
2. Open the dashboard, click "Connect Claude" (or ChatGPT / Gemini). A real Chromium window opens. Log in like you normally would. altkey captures the session cookie and stores it encrypted.
3. altkey mints you a fake `sk-...` key.
4. Point any OpenAI-compatible tool (Cursor, Continue, OpenWebUI, etc.) at `http://127.0.0.1:8787/v1` with that key.
5. The model name routes the request — `claude-*` → Claude session, `gpt-*` → ChatGPT, `gemini-*` → Gemini.

## Setup

```powershell
cd C:\Users\gsent\Desktop\altkey
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -e .
playwright install chromium
python -m app.main
```

Open http://127.0.0.1:8787 — connect accounts, copy your key.

## Status

- [x] FastAPI scaffold + OpenAI-compatible endpoints
- [x] SQLite + Fernet cookie store
- [x] Playwright session harvester
- [x] Claude provider (claude.ai backend)
- [ ] ChatGPT provider (stub)
- [ ] Gemini provider (stub)
- [ ] Dashboard polish

This repo is the OSS self-host scaffold. The hosted multi-tenant SaaS design lives in `docs/superpowers/specs/2026-05-27-altkey-design.md`.
