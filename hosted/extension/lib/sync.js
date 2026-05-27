// Sync layer — encrypts a SessionBlob with K_session and POSTs to the server.

import { getMeta, getKSession, getSyncHmac, isLocked } from "./state.js";
import { encrypt, hmacSha256, bytesToB64, utf8ToBytes, concatBytes } from "./crypto.js";

export async function uploadSession(provider, sessionBlob) {
  if (isLocked()) throw new Error("vault locked");
  const meta = await getMeta();
  if (!meta) throw new Error("not onboarded");
  const kSess = getKSession();
  const hmacKey = getSyncHmac();

  const plaintext = utf8ToBytes(JSON.stringify(sessionBlob));
  const { ciphertext, nonce } = encrypt(kSess, plaintext);
  const capturedAt = Math.floor(Date.now() / 1000);

  const ts = new Uint8Array(8);
  new DataView(ts.buffer).setBigUint64(0, BigInt(capturedAt), false);
  const signed = concatBytes(ciphertext, nonce, ts, utf8ToBytes(provider));
  const sig = await hmacSha256(hmacKey, signed);

  const body = {
    user_id: meta.user_id,
    provider,
    ciphertext_b64: bytesToB64(ciphertext),
    nonce_b64: bytesToB64(nonce),
    captured_at: capturedAt,
    signature_b64: bytesToB64(sig),
  };

  const r = await fetch(`${meta.server_origin}/sync/upload`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) throw new Error(`sync upload ${r.status}: ${await r.text()}`);
  return await r.json();
}
