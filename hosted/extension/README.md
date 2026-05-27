# altkey browser extension (MV3)

Captures your Claude / ChatGPT / Gemini session cookies, encrypts them locally with your passphrase, and uploads ciphertext blobs to the altkey server.

## Vendor bundle (required before loading)

Two files need to be vendored into `lib/vendor/`:

1. **`noble-ciphers.js`** — bundle `@noble/ciphers/chacha` as an ES module. From a tooling project:

   ```bash
   npm i @noble/ciphers
   npx esbuild --bundle --format=esm --minify \
     --define:global=globalThis \
     <(echo "export { xchacha20poly1305 } from '@noble/ciphers/chacha';") \
     > lib/vendor/noble-ciphers.js
   ```

2. **`argon2-bundled.js`** — bundle `argon2-browser` (Argon2id WASM):

   ```bash
   npm i argon2-browser
   npx esbuild --bundle --format=esm --minify --loader:.wasm=base64 \
     node_modules/argon2-browser/lib/argon2.js > lib/vendor/argon2-bundled.js
   ```

   Or use a prebuilt MV3-compatible Argon2 module like
   `hash-wasm` (`argon2id` export) if WASM loading restrictions bite.

## Load unpacked (dev)

1. `chrome://extensions` → toggle Developer Mode → **Load unpacked** → select `hosted/extension/`.
2. Click the extension icon.
3. Choose a passphrase (12+ chars). Save the recovery code shown.
4. Log into claude.ai / chatgpt.com / gemini.google.com in normal tabs.
5. The extension will silently capture and upload encrypted cookies within 5s.

## Server URL

Default: `https://api.altkey.app`. Override by editing `SERVER_ORIGIN` in `popup.js` before packaging. For local dev, point at `http://127.0.0.1:8787` (the local FastAPI proxy doesn't speak the hosted sync protocol — only the Cloudflare Workers do).

## Crypto invariants

- Passphrase → `K_user` (Argon2id, m=64MB, t=3) — never persists, never leaves the device.
- `K_session` (random 32 bytes) is generated once, wrapped under `K_user`, and stored in `chrome.storage.local` (encrypted).
- Cookie blobs are encrypted with `K_session` (XChaCha20-Poly1305, random 24-byte nonce per blob) before upload.
- Upload is HMAC-SHA256 signed with `HKDF(K_user, "sync-hmac")` so the server can verify authenticity without seeing `K_user`.

## Lock behavior

- Vault auto-locks after 5 minutes of popup inactivity.
- Vault locks on browser restart (service worker memory is volatile).
- Click "Lock" in the popup to lock manually.

## Permissions

- `cookies` — to read session cookies for the three providers.
- `storage` — to persist `altkey_meta` (salt, verifier, ciphertext blobs).
- `alarms` — heartbeat re-sync every 30 minutes.

No content scripts. No `tabs` permission. No `webRequest`.
