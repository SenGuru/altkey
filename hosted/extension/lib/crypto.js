// altkey extension crypto. Matches workers/src/crypto.ts semantically.
//
// We use:
//   - argon2-browser (WASM) for Argon2id  → K_user from passphrase
//   - WebCrypto SubtleCrypto              → HMAC-SHA256, SHA-256, HKDF-derived keys
//   - js-implementation of XChaCha20-Poly1305 via @noble/ciphers (browser ESM build)
//
// IMPORTANT: this file is loaded as a module from the extension and expects
// `lib/vendor/noble-ciphers.js` and `lib/vendor/argon2-bundled.js` to be
// shipped in the extension package. See popup.html for module type.

import { xchacha20poly1305 } from "./vendor/noble-ciphers.js";

export const KEY_BYTES = 32;
export const NONCE_BYTES = 24;
export const SALT_BYTES = 16;

export function randomBytes(n) {
  const b = new Uint8Array(n);
  crypto.getRandomValues(b);
  return b;
}

export function bytesToHex(b) {
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

export function hexToBytes(h) {
  const out = new Uint8Array(h.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function bytesToB64(b) {
  let s = "";
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s);
}

export function b64ToBytes(s) {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function utf8ToBytes(s) {
  return new TextEncoder().encode(s);
}

export function bytesToUtf8(b) {
  return new TextDecoder().decode(b);
}

export function concatBytes(...arrs) {
  const total = arrs.reduce((s, a) => s + a.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const a of arrs) {
    out.set(a, o);
    o += a.length;
  }
  return out;
}

// Argon2id is supplied by argon2-browser; load lazily so popup boots fast.
let _argon2Mod = null;
async function argon2() {
  if (_argon2Mod) return _argon2Mod;
  _argon2Mod = await import("./vendor/argon2-bundled.js");
  return _argon2Mod;
}

export async function deriveKUser(passphrase, salt) {
  const m = await argon2();
  const res = await m.argon2.hash({
    pass: passphrase,
    salt,
    time: 3,
    mem: 64 * 1024,
    parallelism: 1,
    type: m.argon2.ArgonType.Argon2id,
    hashLen: KEY_BYTES,
  });
  return new Uint8Array(res.hash);
}

export async function sha256(data) {
  const buf = await crypto.subtle.digest("SHA-256", data);
  return new Uint8Array(buf);
}

// HKDF-Expand using SubtleCrypto. Salt empty, info = utf8(label) || optionalContext.
export async function hkdfKey(ikm, label, context = new Uint8Array(0)) {
  const baseKey = await crypto.subtle.importKey("raw", ikm, "HKDF", false, ["deriveBits"]);
  const info = concatBytes(utf8ToBytes(label), context);
  const bits = await crypto.subtle.deriveBits(
    { name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info },
    baseKey,
    KEY_BYTES * 8,
  );
  return new Uint8Array(bits);
}

export async function hmacSha256(key, message) {
  const k = await crypto.subtle.importKey(
    "raw",
    key,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("HMAC", k, message);
  return new Uint8Array(sig);
}

export function encrypt(key, plaintext, aad) {
  if (key.length !== KEY_BYTES) throw new Error("key must be 32 bytes");
  const nonce = randomBytes(NONCE_BYTES);
  const cipher = xchacha20poly1305(key, nonce, aad);
  const ct = cipher.encrypt(plaintext);
  return { ciphertext: ct, nonce };
}

export function decrypt(key, ciphertext, nonce, aad) {
  const cipher = xchacha20poly1305(key, nonce, aad);
  return cipher.decrypt(ciphertext);
}

export function zero(buf) {
  if (buf instanceof Uint8Array) buf.fill(0);
}
