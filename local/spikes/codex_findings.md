# ChatGPT Codex OAuth — spike findings (2026-05-29)

VIABLE on a ChatGPT **Plus** account. Verified: returns a real completion.

## Auth
- Token source: `~/.codex/auth.json` → `tokens.access_token` / `refresh_token` / `account_id`.
- Access token expires (~hours). Refresh:
  - `POST https://auth.openai.com/oauth/token`
  - `{grant_type:"refresh_token", refresh_token, client_id:"app_EMoamEEZ73f0CkXaXp7hrann", scope:"openid profile email offline_access"}`
  - → `{access_token, ...}`. Refresh token is long-lived (worked after ~2 months).

## Request
- `POST https://chatgpt.com/backend-api/codex/responses`
- Headers: `Authorization: Bearer <access_token>`, `chatgpt-account-id: <account_id>`,
  `OpenAI-Beta: responses=experimental`, `originator: codex_cli_rs`,
  `User-Agent: codex_cli_rs/0.135.0`, `Content-Type: application/json`.
- Body: `{model, instructions, input:[{type:"message",role,content:[{type:"input_text",text}]}], stream:true, store:false}`
- **`stream` MUST be true** (else 400 "Stream must be set to true").

## Models (this Plus account) — verified live 2026-05-29
- ✅ `gpt-5.5`  — WORKS. Current frontier (released 2026-04-23). **Default.**
- ✅ `gpt-5.2`  — works (previous gen)
- ❌ `gpt-5.5-codex`, `gpt-5.5-thinking`, `gpt-5.5-instant` — Pro/variant-gated
- ❌ `gpt-5.5-codex`, `gpt-5.1-codex-mini`, `gpt-5.1`, `gpt-5` — not supported
- ❌ `gpt-5.6` — not released
- Rule: plain frontier general models (`gpt-5.5`, `gpt-5.2`) work on Plus; `-codex`/`-thinking`/`-instant` variants are gated.
- Codex CLI updated 0.88.0 → 0.135.0 during the spike (good hygiene; not required — `gpt-5.5` works regardless).

## Response (SSE)
- Lines `data: {...}`. Text deltas: `{"type":"response.output_text.delta","delta":"..."}`.
- Completion: `response.completed`. Items: `response.output_item.added/done`.

## Vision (VERIFIED 2026-05-29)
- ✅ Works. Message content: `{"type":"input_image","image_url":"data:image/png;base64,..."}`.
- Test: identified the Frederick Douglass image correctly.

## Image generation (VERIFIED 2026-05-29)
- ✅ Works. Add tool `{"type":"image_generation"}` to the request.
- SSE events: `response.image_generation_call.in_progress` → `.generating` →
  `.partial_image` (field `partial_image_b64`, full base64 PNG, ~1.5MB) → done.
- Map to OpenAI `/v1/images/generations` (return b64_json) for the gateway.
