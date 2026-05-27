# altkey — local self-host

Local OpenAI-compatible proxy that routes requests through your **own** Claude Pro, ChatGPT Plus, and Gemini Advanced sessions. One proxy, one fake `sk-alt-*` key, three upstreams chosen by model name.

> Violates each provider's ToS. Personal use only, your own accounts only, never bind to a public interface.

## Setup

```powershell
cd C:\Users\gsent\Desktop\altkey\local
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -e .
playwright install chromium
python -m app.main
```

Then open <http://127.0.0.1:8787>.

1. Click **Connect Claude** (or ChatGPT / Gemini). A real Chromium window opens.
2. Log in normally. altkey captures the session cookie when it sees you land on the post-login page.
3. Mint an API key in the dashboard.
4. Point any OpenAI-compatible tool at `http://127.0.0.1:8787/v1` with the minted key.

## Model name → provider routing

| Prefix | Routes to | Examples |
|---|---|---|
| `claude-*` | Claude (claude.ai) | `claude-sonnet-4-5`, `claude-opus-4-5`, `claude-3-5-haiku-20241022` |
| `gpt-*`, `o1`, `o3`, `o4-*`, `chatgpt-*` | ChatGPT (chatgpt.com) | `gpt-4o`, `gpt-4-1-mini`, `o3`, `chatgpt-4o-latest` |
| `gemini-*` | Gemini (gemini.google.com) | `gemini-2.5-pro`, `gemini-2.5-flash`, `gemini-1.5-pro` |

## Endpoints

- `GET  /v1/models` — list known models (requires Bearer key)
- `POST /v1/chat/completions` — OpenAI-compatible chat completions (streaming + non-streaming)
- `GET  /` — dashboard (no auth — bound to 127.0.0.1 only)

## How auth works

- `Authorization: Bearer sk-alt-...` checked against SQLite at `~/.altkey/store.db`.
- Cookies are encrypted at rest with Fernet (key in OS keyring under service `altkey`).
- The proxy decrypts cookies in memory per request and never logs them.

## Cookie expiry

| Provider | Approx. cookie lifetime | What to do when it expires |
|---|---|---|
| Claude | ~30 days | Click **Connect Claude** again; just re-login |
| ChatGPT | ~7 days (session token); Cloudflare clearance hours | Same — Connect ChatGPT, log in |
| Gemini | ~14 days (PSIDTS rotates) | Same — Connect Gemini, log in |

When cookies expire you'll see `502 session expired` from the proxy. Re-run Connect for the affected provider.

## Tests

```powershell
python -m pytest tests/ -v
```

## Caveats

- **ChatGPT + Gemini are best-effort reverse-engineered.** Their internal request shapes change ~quarterly. If a provider returns 4xx unexpectedly, the request payload likely needs an update in `app/providers/<provider>.py`.
- **Conversation deletion:** Claude deletes the conversation server-side after each request so your sidebar doesn't fill with API calls. ChatGPT and Gemini do not — completed conversations will appear in their respective histories.
- **No mock for tests:** unit tests cover translators and storage. End-to-end against live providers requires you to be logged in locally — run `python -m app.main`, connect a provider, and `curl` the endpoint.

## Layout

```
local/
├── app/
│   ├── main.py              FastAPI app + admin endpoints
│   ├── store.py             SQLite + Fernet vault
│   ├── harvester.py         Playwright login flow
│   ├── dashboard.html       single-page UI
│   └── providers/
│       ├── __init__.py      model-prefix routing
│       ├── claude.py        claude.ai → OpenAI
│       ├── chatgpt.py       chatgpt.com → OpenAI
│       └── gemini.py        gemini.google.com → OpenAI
└── tests/
    ├── test_store.py
    ├── test_providers_translate.py
    └── test_harvester_smoke.py
```

The hosted multi-tenant version lives in `../hosted/`. The design spec is `../docs/superpowers/specs/2026-05-27-altkey-design.md`.
