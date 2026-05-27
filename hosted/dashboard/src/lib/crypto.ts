// Dashboard-side crypto. Mirrors the extension primitives. Used only to
// derive K_user from a passphrase + compute the SHA256 verifier we send
// to /api/unlock. The dashboard never holds K_session or decrypts cookies.

const KEY_BYTES = 32;
const SALT_BYTES = 16;

export function b64ToBytes(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function bytesToB64(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s);
}

export async function sha256(data: Uint8Array): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", data));
}

// Pure-JS Argon2id is heavy. The dashboard imports the user's salt from the
// extension (synced via window.postMessage or a deep-link signin flow).
// For now we treat unlock as "you must use the extension to set the cookie";
// the dashboard reads that cookie and queries the API.
//
// If you want passphrase entry directly in the dashboard, ship the same
// argon2-browser bundle as the extension. See ../../extension/lib/crypto.js.

export async function deriveKUserViaWasm(passphrase: string, salt: Uint8Array): Promise<Uint8Array> {
  // Lazy import — only when needed.
  const mod = await import("argon2-browser");
  const res = await (mod as any).argon2.hash({
    pass: passphrase,
    salt,
    time: 3,
    mem: 64 * 1024,
    parallelism: 1,
    type: (mod as any).argon2.ArgonType.Argon2id,
    hashLen: KEY_BYTES,
  });
  return new Uint8Array(res.hash);
}

export { KEY_BYTES, SALT_BYTES };
