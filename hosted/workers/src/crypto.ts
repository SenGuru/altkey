// altkey hosted crypto primitives. See spec §3.

import { xchacha20poly1305 } from "@noble/ciphers/chacha";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";
import { hmac } from "@noble/hashes/hmac";
import { utf8ToBytes, bytesToHex, hexToBytes } from "@noble/hashes/utils";

export const NONCE_BYTES = 24;
export const KEY_BYTES = 32;

export function randomBytes(n: number): Uint8Array {
  const buf = new Uint8Array(n);
  crypto.getRandomValues(buf);
  return buf;
}

export function concatBytes(...arrs: Uint8Array[]): Uint8Array {
  const total = arrs.reduce((s, a) => s + a.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const a of arrs) {
    out.set(a, o);
    o += a.length;
  }
  return out;
}

export function timingSafeEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= (a[i] ?? 0) ^ (b[i] ?? 0);
  return diff === 0;
}

// Encrypt with XChaCha20-Poly1305. Returns ciphertext (incl. 16-byte tag) and nonce.
export function encrypt(key: Uint8Array, plaintext: Uint8Array, aad?: Uint8Array): {
  ciphertext: Uint8Array;
  nonce: Uint8Array;
} {
  if (key.length !== KEY_BYTES) throw new Error("key must be 32 bytes");
  const nonce = randomBytes(NONCE_BYTES);
  const cipher = xchacha20poly1305(key, nonce, aad);
  const ciphertext = cipher.encrypt(plaintext);
  return { ciphertext, nonce };
}

export function decrypt(
  key: Uint8Array,
  ciphertext: Uint8Array,
  nonce: Uint8Array,
  aad?: Uint8Array,
): Uint8Array {
  if (key.length !== KEY_BYTES) throw new Error("key must be 32 bytes");
  if (nonce.length !== NONCE_BYTES) throw new Error("nonce must be 24 bytes");
  const cipher = xchacha20poly1305(key, nonce, aad);
  return cipher.decrypt(ciphertext); // throws on tag mismatch
}

// HKDF-Expand-Extract → 32-byte key.
export function hkdfKey(ikm: Uint8Array, info: string, salt?: Uint8Array): Uint8Array {
  return hkdf(sha256, ikm, salt ?? new Uint8Array(0), utf8ToBytes(info), KEY_BYTES);
}

// Per-request, per-user K_worker = HKDF(CF_SECRET, user_id, "kworker").
// Reduces blast radius: a memory dump captures K_worker only for users
// with currently in-flight requests on that isolate.
export function deriveKWorker(cfSecretHex: string, userId: string): Uint8Array {
  const ikm = hexToBytes(cfSecretHex);
  return hkdfKey(ikm, "kworker", utf8ToBytes(userId));
}

// Wrap K_session under K_worker for storage in users.proxy_k_session.
export function wrapKSessionForProxy(kSession: Uint8Array, kWorker: Uint8Array): {
  ciphertext: Uint8Array;
  nonce: Uint8Array;
} {
  return encrypt(kWorker, kSession);
}

export function unwrapKSessionFromProxy(
  wrapped: Uint8Array,
  nonce: Uint8Array,
  kWorker: Uint8Array,
): Uint8Array {
  return decrypt(kWorker, wrapped, nonce);
}

// HMAC-SHA256 with the sync HMAC key (HKDF(K_user, "sync-hmac")).
export function hmacSign(key: Uint8Array, message: Uint8Array): Uint8Array {
  return hmac(sha256, key, message);
}

export function hmacVerify(key: Uint8Array, message: Uint8Array, sig: Uint8Array): boolean {
  return timingSafeEqual(hmacSign(key, message), sig);
}

// SHA-256 helpers.
export function sha256Bytes(data: Uint8Array): Uint8Array {
  return sha256(data);
}

export function sha256Hex(data: Uint8Array): string {
  return bytesToHex(sha256(data));
}

// API-key hashing — keys are 256-bit entropy so fast hash is fine.
export function hashApiKey(key: string): Uint8Array {
  return sha256(utf8ToBytes(key));
}

// Zeroize a buffer in place. JS GC may copy under the hood — best effort only.
export function zero(buf: Uint8Array): void {
  buf.fill(0);
}

// Convenience: serialize and store as opaque blob.
// Layout: [24-byte nonce][ciphertext]
export function pack(ciphertext: Uint8Array, nonce: Uint8Array): Uint8Array {
  return concatBytes(nonce, ciphertext);
}

export function unpack(packed: Uint8Array): { ciphertext: Uint8Array; nonce: Uint8Array } {
  if (packed.length < NONCE_BYTES + 16) throw new Error("packed blob too short");
  return {
    nonce: packed.slice(0, NONCE_BYTES),
    ciphertext: packed.slice(NONCE_BYTES),
  };
}

export { bytesToHex, hexToBytes, utf8ToBytes };
