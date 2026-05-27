# altkey

One OpenAI-compatible API key, routed through your **own** Claude Pro, ChatGPT Plus, and Gemini Advanced subscriptions.

> Violates the Terms of Service of every upstream provider. Personal use only.

This repository contains two parallel tracks:

| Track | Folder | Status | When to use |
|---|---|---|---|
| **Local self-host** | [`local/`](./local) | Runnable today | You want it on your own machine, no setup other than `pip install` |
| **Hosted SaaS** | [`hosted/`](./hosted) | Code-complete; needs secret config to deploy | You're operating a multi-tenant service |

The hosted track is designed against the threat model documented in
[`docs/superpowers/specs/2026-05-27-altkey-design.md`](./docs/superpowers/specs/2026-05-27-altkey-design.md)
— see §3 for the crypto + threat model and §5 for data flows.

---

## Quickstart (local)

```powershell
cd local
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -e ".[dev]"
playwright install chromium
python -m app.main
```

Open <http://127.0.0.1:8787>, click Connect for each provider, mint a key.

```bash
curl http://127.0.0.1:8787/v1/chat/completions \
  -H "Authorization: Bearer sk-alt-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-5","messages":[{"role":"user","content":"hi"}]}'
```

## Quickstart (hosted)

See [`hosted/ops/deploy.md`](./hosted/ops/deploy.md). Short version:

```bash
cd hosted/workers && npm install
wrangler d1 create altkey
npm run migrate:remote
openssl rand -hex 32 | wrangler secret put CF_SECRET
# ...other secrets per ops/deploy.md
wrangler deploy

cd ../dashboard && npm install && npm run build
# deploy via Cloudflare Pages
```

---

## Repository layout

```
altkey/
├── docs/
│   └── superpowers/
│       ├── specs/2026-05-27-altkey-design.md     full design spec (§1-§14)
│       └── plans/2026-05-27-altkey-implementation.md   phased build plan
├── local/                                        Python FastAPI proxy
│   ├── app/
│   │   ├── main.py
│   │   ├── store.py
│   │   ├── harvester.py
│   │   ├── dashboard.html
│   │   └── providers/{claude,chatgpt,gemini}.py
│   └── tests/                                    30 unit tests passing
└── hosted/                                       Cloudflare Workers SaaS
    ├── workers/                                  Hono + D1 + R2 backend
    ├── extension/                                MV3 Chrome/Firefox extension
    ├── dashboard/                                Astro static site
    └── ops/
        ├── canary.sh                             weekly warrant canary
        └── deploy.md                             prod deploy runbook
```

## Status

| Component | Status |
|---|---|
| Local — Claude provider | ✅ working |
| Local — ChatGPT provider | ✅ working (best-effort RE, subject to provider rotation) |
| Local — Gemini provider | ✅ working (best-effort RE) |
| Local — Playwright harvester | ✅ working (all three providers) |
| Local — Fernet vault + dashboard | ✅ working |
| Local — tests | ✅ 30/30 passing |
| Hosted — Workers (proxy/control/sync/webhook/canary) | ✅ code-complete |
| Hosted — Crypto (XChaCha20 + Argon2id + K_worker) | ✅ + threat-model regression test |
| Hosted — D1 schema | ✅ |
| Hosted — Providers (TS port) | ✅ |
| Hosted — Polar + NOWPayments billing | ✅ |
| Hosted — MV3 extension | ✅ (needs vendor bundles + icons) |
| Hosted — Astro dashboard | ✅ |
| Hosted — Deploy runbook | ✅ |

## License

TBD. Until set, all rights reserved; the code is published for transparency (especially the crypto) and is not licensed for redistribution.

## Disclaimer

This software exists to use AI subscriptions you've already paid for inside tools that expect API access. The author makes no warranty and assumes no liability. By using it you accept:

- The risk of your provider account being suspended.
- The fact that this violates each provider's Terms of Service.
- That the hosted service may be shut down by the author or by the providers at any time, with refunds prorated.

The OSS self-host version (`local/`) cannot be unilaterally taken offline — once you have a copy, you have it.
