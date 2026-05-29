# altkey transparent intercept (personal machine only)

Makes apps that **hardcode** `api.openai.com` / `api.anthropic.com` (no base-URL
setting) transparently hit altkey instead — running on your own subscriptions —
with zero config in the app.

## ⚠️ Read this first

This installs a **local root Certificate Authority** into your Windows trust
store and redirects provider API domains to `127.0.0.1`. That CA's private key
(`ca.key`) lives in this folder. **Anyone who steals `ca.key` can impersonate
any HTTPS site to your machine.** Only do this on a personal machine you control,
keep `ca.key` private, and run `teardown.ps1` when you're done.

This only works for apps that don't pin certificates (most don't). Cert-pinning
apps will refuse the connection — that's their anti-MITM working as intended.

## How it works

1. A local CA signs a cert valid for `api.openai.com`, `api.anthropic.com`, etc.
2. The CA is installed in your Windows trust store, so your machine trusts that cert.
3. hosts entries point those domains at `127.0.0.1`.
4. altkey listens on 443 with the cert and accepts any key.
5. An app calling `https://api.openai.com/v1/chat/completions` lands on altkey,
   which runs the request on your Claude/ChatGPT subscription and replies in the
   format the app expects. The app can't tell the difference.

## Setup

```powershell
cd C:\Users\gsent\Desktop\altkey\local

# 1. Generate the CA + cert (no admin)
python transparent\gen_certs.py

# 2. Install CA + hosts entries (Administrator PowerShell)
.\transparent\setup.ps1
# Approve the Windows security prompt if shown.

# 3. Launch altkey in transparent mode (Administrator — needs port 443)
.\transparent\run-transparent.ps1
```

Now point any hardcoded-endpoint app at its provider as normal (any key). Model
names route: `gpt-*` → your ChatGPT sub, `claude-*` → your Claude sub.

## Undo

```powershell
.\transparent\teardown.ps1   # Administrator — removes CA + hosts entries
```

## Notes / limits

- **Auth is open in transparent mode** (any key accepted), because the app sends
  whatever key it was built with. Keep altkey bound to localhost.
- Only OpenAI-wire (`/v1/chat/completions`) and Anthropic-wire (`/v1/messages`)
  are intercepted. Google's Gemini API uses a different wire format and is not
  intercepted here (use the configured-endpoint path for Gemini).
- If an app still bypasses it, it's pinning certs — nothing to be done short of
  patching the app.
