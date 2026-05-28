---
title: altkey relay test
emoji: 🔑
colorFrom: blue
colorGreen: gray
sdk: docker
app_port: 7860
pinned: false
---

# altkey relay — datacenter IP test

Temporary Hugging Face Space to test one question: **does the subscription
relay still work when the request comes from a datacenter IP instead of the
user's home IP?**

This is not the product — it's a probe. If Claude/ChatGPT/Gemini accept the
relayed request from HF's datacenter, the bare hosted relay is viable without
residential proxies. If they 403/ban, proxies are mandatory.

## Required Space secrets

Set under Settings → Variables and secrets:

- `ALTKEY_ADMIN_TOKEN` — random string; gates all `/admin/*` endpoints.
- `ALTKEY_FERNET_KEY` — a Fernet key (generate with
  `python -c "from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())"`).

## Test flow

1. Deploy this Space (Docker SDK).
2. From your machine, push your real Claude cookie to it:
   `POST /admin/import` with header `X-Admin-Token: <token>`.
3. Mint a key: `POST /admin/keys` with the same header.
4. Fire `POST /v1/chat/completions` with that key → the Space relays to
   claude.ai **from HF's IP**. Observe success / 403 / ban.

⚠️ This violates provider ToS. Test with your own account only. Tear the Space
down afterward.
