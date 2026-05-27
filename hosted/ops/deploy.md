# altkey deploy runbook

Step-by-step to take this scaffold from `git clone` to live production.

## Prerequisites

- Cloudflare account (free tier is enough to start)
- Polar.sh account (apply at https://polar.sh — onboarding takes 1–2 weeks)
- NOWPayments account (https://nowpayments.io — usually approves same-day)
- A domain. Use a pseudonymity-friendly registrar like Njalla or 1984 Hosting.
- Node 20+ and `wrangler` installed (`npm i -g wrangler`)

## 1. Workers backend

```bash
cd hosted/workers
npm install

# Create D1 database
wrangler d1 create altkey
# Paste the printed database_id into wrangler.toml under [[d1_databases]].

# Apply schema
npm run migrate:remote

# Generate and set the CF_SECRET (used to derive K_worker).
# This is the master server-side secret; rotate every 30 days.
openssl rand -hex 32 | wrangler secret put CF_SECRET

# Set billing secrets (obtain from Polar / NOWPayments dashboards).
wrangler secret put POLAR_API_KEY
wrangler secret put POLAR_WEBHOOK_SECRET
wrangler secret put NOWPAYMENTS_API_KEY
wrangler secret put NOWPAYMENTS_IPN_SECRET

# Generate and set the warrant canary ed25519 key.
openssl genpkey -algorithm ed25519 -out /tmp/canary.pem
openssl pkey -in /tmp/canary.pem -text -noout | grep -A4 priv | tail -3 | tr -d ' :\n' | wrangler secret put CANARY_PRIVATE_KEY
openssl pkey -in /tmp/canary.pem -text -noout | grep -A4 pub | tail -3 | tr -d ' :\n' > /tmp/canary.pub
# Paste the pub hex into wrangler.toml as CANARY_PUBLIC_KEY (vars section).
rm /tmp/canary.pem

# Deploy
wrangler deploy
```

Your Worker is now at `https://altkey.<your-cf-subdomain>.workers.dev`. Add a custom route at `api.altkey.app` via Cloudflare DNS + Worker route.

## 2. Polar configuration

1. Create a "Pro" product in Polar at $5/mo recurring USD.
2. Copy the product ID. Hardcode it in `workers/src/billing/polar.ts:createCheckout` (or pull from D1 config).
3. In Polar dashboard → Webhooks: add `https://api.altkey.app/wh/polar`, subscribe to `subscription.created`, `subscription.canceled`, `subscription.refunded`, `checkout.completed`. Copy the signing secret into `POLAR_WEBHOOK_SECRET`.

## 3. NOWPayments configuration

1. NOWPayments dashboard → IPN settings: callback URL `https://api.altkey.app/wh/nowpayments`. Copy the IPN secret into `NOWPAYMENTS_IPN_SECRET`.
2. Whitelist USD as a billing currency.

## 4. Dashboard (Cloudflare Pages)

```bash
cd hosted/dashboard
npm install
npm run build

# Deploy to Cloudflare Pages. Connect your git repo via the CF dashboard,
# point root directory at hosted/dashboard, build command `npm run build`,
# output directory `dist`.
#
# Set env var PUBLIC_API_BASE=https://api.altkey.app.
# Add custom domain altkey.app.
```

## 5. Extension distribution

1. Bundle vendor files per `extension/README.md`.
2. Zip the `extension/` directory.
3. Chrome Web Store developer console: $5 one-time fee, upload, fill listing. Review takes 1–4 weeks.
4. Firefox AMO: free, upload, review takes 3–7 days.
5. While reviews are pending, link the github release for sideload.

## 6. Canary cron

Run `ops/canary.sh` weekly. Options:
- GitHub Actions cron (cheapest)
- Cloudflare Cron Trigger on a separate Worker
- Local cron on a server you control

The shell script needs `CANARY_PRIVATE_KEY` env var and `wrangler` available to upload to R2.

## 7. R2 bucket for canary

```bash
wrangler r2 bucket create altkey-canary
```

Update the Worker `canary.ts` to fetch from R2 instead of returning the bootstrap text. (Optional refinement; bootstrap works initially.)

## 8. Monitoring

- BetterUptime (https://betteruptime.com): monitor `https://api.altkey.app/healthz` every 60s. Free tier covers it.
- Sentry: add DSN as a secret, wire `beforeSend` cookie scrubber in `src/log.ts`.
- ntfy.sh: push alerts on `upstream.shape_changed` and `crypto.decrypt_failed`. The Worker should POST to your private ntfy topic.

## 9. Kill switch test (monthly)

```bash
# 1. Flip the kill flag (set a `vars.SERVICE_KILLED=true` and redeploy).
wrangler deploy --var SERVICE_KILLED:true

# 2. Verify /v1/* returns 503 with the OSS-redirect message.
curl -i https://api.altkey.app/v1/models -H "Authorization: Bearer sk-alt-fake"

# 3. Unflip.
wrangler deploy
```

(The kill switch handler isn't in the scaffold by default — add a `c.env.SERVICE_KILLED` check at the top of the proxy route.)

## 10. Day-one checklist

- [ ] D1 schema applied, `CF_SECRET` set, deploy green
- [ ] Polar webhook delivers test event successfully
- [ ] NOWPayments IPN delivers test event
- [ ] Canary script committed to GitHub Actions and runs nightly
- [ ] Extension uploaded for review on both stores
- [ ] Dashboard deployed to Pages with custom domain
- [ ] Internal smoke test: sign up via extension → mint key → curl `/v1/chat/completions` → 200
- [ ] BetterUptime monitor green
