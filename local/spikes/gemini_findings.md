# Gemini CLI OAuth — spike findings (2026-05-30)

VIABLE on a Gemini **Pro** account via Google Code Assist. Verified chat works.

## Auth
- Creds: `~/.gemini/oauth_creds.json` → `access_token` / `refresh_token` / `expiry_date` (ms).
- Token expires ~1h. Refresh via Google OAuth:
  - `POST https://oauth2.googleapis.com/token`
  - `{client_id, client_secret, refresh_token, grant_type:"refresh_token"}`
  - gemini-cli public client: `681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com`
    secret `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` (confirm in build).

## Flow (Code Assist, NOT the API-key generativelanguage endpoint)
- Base: `https://cloudcode-pa.googleapis.com/v1internal`
- 1) `POST :loadCodeAssist` `{metadata:{pluginType:"GEMINI"}}` → `currentTier` (standard-tier)
     + `cloudaicompanionProject` (e.g. `rapid-hull-h00jl`). If absent, call `:onboardUser`.
- 2) `POST :streamGenerateContent?alt=sse`
     body: `{model, project, request:{contents:[{role,parts:[{text}]}]}}`
     headers: `Authorization: Bearer <tok>`, `Content-Type: application/json`, `User-Agent: GeminiCLI/0.44.1`
- Response SSE: `data: {response:{candidates:[{content:{parts:[{text}]}}]}}`.

## Verified (2026-05-30)
- ✅ chat: `gemini-2.5-pro` → "gemini oauth works"
- ✅ vision: image as `parts:[{inlineData:{mimeType, data:<b64>}}]` → identified Frederick Douglass
- ❌ image gen: image models (`gemini-2.5-flash-image`, `gemini-3-pro-image-preview`) → 404
  on Code Assist. Image generation is NOT available via this OAuth path; ChatGPT covers image gen.
- Tier: standard-tier "Gemini Code Assist — Unlimited … most powerful Gemini models"

## TODO in build
- Tool calling: `request.tools:[{functionDeclarations:[...]}]`; response `functionCall` parts → OpenAI tool_calls.
- Cache project id (from loadCodeAssist) + refresh token (Google OAuth).
- Model detection: confirm current model names (gemini-2.5-pro works; try gemini-3-* in build).
