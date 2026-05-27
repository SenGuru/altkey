# altkey — hosted SaaS (`hosted/`)

This is the **production-grade multi-tenant** version: Cloudflare Workers backend, MV3 browser extension, Astro static dashboard, end-to-end-ish encrypted cookie vault, Polar.sh + NOWPayments billing.

It is **code-complete**. To go live you need to:

1. Configure secrets (D1 ID, `CF_SECRET`, Polar/NOWPayments keys, canary key).
2. Deploy the three pieces: workers, dashboard, extension.

See [`ops/deploy.md`](./ops/deploy.md) for the exact commands.

> If you just want to *test the proxy logic without spinning up Cloudflare infra*, use `../local/` instead — it runs in one Python process on `127.0.0.1:8787`.

---

## Architecture (recap)

```
Browser  ──┬──► Extension (MV3) ──HTTPS──► Workers /sync     ─► D1 (ciphertext)
           │                                                  
           └──► Dashboard (Astro) ──HTTPS──► Workers /api     ─► D1
                                                              
External   ──Bearer key──► Workers /v1 ─► decrypt in RAM ─► claude.ai / chatgpt.com / gemini.google.com
client                                    (per-request,        (with user's cookies)
                                           never logged)
```

The crypto and threat model are documented in [`../docs/superpowers/specs/2026-05-27-altkey-design.md`](../docs/superpowers/specs/2026-05-27-altkey-design.md) §3 — every property there is enforced in `workers/src/crypto.ts` and verified by `workers/src/crypto.test.ts`.

---

## Layout

```
hosted/
├── package.json              workspace root
├── workers/                  Cloudflare Workers (Hono + D1 + R2)
│   ├── wrangler.toml         deploy config
│   ├── migrations/0001_init.sql
│   └── src/
│       ├── index.ts          route splitter
│       ├── crypto.ts         XChaCha20 + HKDF + HMAC + K_worker
│       ├── crypto.test.ts    threat-model regression test
│       ├── db.ts             typed D1 queries
│       ├── log.ts            scrubbing logger
│       ├── auth.ts           bearer key + session cookie
│       ├── quota.ts          free-tier + abuse caps
│       ├── routes/           proxy, control, sync, webhook, canary
│       ├── providers/        claude, chatgpt, gemini (TS port)
│       ├── billing/          polar, nowpayments
│       └── utils/            openai chunk + SSE writer
├── extension/                MV3 browser extension
│   ├── manifest.json
│   ├── popup.html / .js / .css
│   ├── background.js         cookies.onChanged → encrypt → POST
│   ├── lib/                  crypto, sync, state (lock/unlock)
│   └── README.md             vendor bundle instructions
├── dashboard/                Astro static site
│   ├── astro.config.mjs
│   └── src/
│       ├── pages/            index, app, billing, docs, canary.txt
│       ├── lib/              api client, crypto for unlock
│       └── styles/global.css
└── ops/
    ├── canary.sh             weekly warrant canary updater
    └── deploy.md             step-by-step prod deploy
```

---

## Required external accounts

| Service | Free? | Purpose | Time to ready |
|---|---|---|---|
| Cloudflare | ✅ | Workers + D1 + Pages + R2 | minutes |
| Polar.sh | ✅ (% fees) | Card billing | 1–2 weeks (review) |
| NOWPayments | ✅ (% fees) | Crypto billing | usually same-day |
| Chrome Web Store | $5 one-time | Extension distribution | 1–4 weeks (review) |
| Firefox AMO | ✅ | Extension distribution | 3–7 days (review) |
| BetterUptime | ✅ free tier | Uptime monitoring | minutes |
| ntfy.sh | ✅ | Push alerts | minutes |
| Sentry | ✅ free tier | Error tracking | minutes |
| Pseudonym registrar (Njalla / 1984) | $ | Domain | minutes |

## Required secrets (set via `wrangler secret put`)

```
CF_SECRET                  hex(32) — derives K_worker for per-user decryption
POLAR_API_KEY              for creating checkouts
POLAR_WEBHOOK_SECRET       HMAC-SHA256 secret for /wh/polar
NOWPAYMENTS_API_KEY        for creating crypto invoices
NOWPAYMENTS_IPN_SECRET     HMAC-SHA512 secret for /wh/nowpayments
CANARY_PRIVATE_KEY         ed25519 hex priv key for canary signing
```

Plus one D1 ID inserted into `wrangler.toml`. That's it.

---

## What's NOT in this scaffold (would need before going live)

- Real product/price IDs in Polar (you create them in Polar dashboard and reference them in `workers/src/routes/control.ts` billing checkout creation).
- Cloudflare Rate Limiting rules (configure in CF dashboard — they're cheaper there than in code).
- Cloudflare Turnstile widget on `/api/signup` (drop the script tag into the extension popup or dashboard).
- Real extension icons (placeholders shipped; replace before submission).
- Vendor bundles for `extension/lib/vendor/` (see `extension/README.md`).

Everything else — crypto, routes, providers, dashboard, billing webhook handling, warrant canary, observability — is implemented.
