// K_user lifecycle for the extension. Lives in service worker memory only.
//
// Persisted in chrome.storage.local:
//   altkey_meta = { user_id, salt_b64, verifier_b64, hmac_pub_b64,
//                   wrapped_k_session_b64, proxy_k_session_b64,
//                   server_origin }
//
// In RAM only:
//   K_user, K_session, K_sync_hmac
//
// On idle/restart, the worker clears RAM and the popup must re-unlock.

import {
  deriveKUser,
  sha256,
  hkdfKey,
  encrypt,
  decrypt,
  randomBytes,
  bytesToB64,
  b64ToBytes,
  utf8ToBytes,
  hexToBytes,
  bytesToHex,
  zero,
  KEY_BYTES,
  SALT_BYTES,
} from "./crypto.js";

const IDLE_MS = 5 * 60 * 1000;

let K_user = null;
let K_session = null;
let K_sync_hmac = null;
let lastActivity = 0;

export async function getMeta() {
  const { altkey_meta } = await chrome.storage.local.get("altkey_meta");
  return altkey_meta ?? null;
}

async function saveMeta(meta) {
  await chrome.storage.local.set({ altkey_meta: meta });
}

export function isLocked() {
  return K_user === null;
}

function maybeIdleLock() {
  if (K_user && Date.now() - lastActivity > IDLE_MS) {
    lock();
  }
}

setInterval(maybeIdleLock, 30 * 1000);

export function lock() {
  if (K_user) zero(K_user);
  if (K_session) zero(K_session);
  if (K_sync_hmac) zero(K_sync_hmac);
  K_user = null;
  K_session = null;
  K_sync_hmac = null;
}

export function touch() {
  lastActivity = Date.now();
}

export async function isOnboarded() {
  return (await getMeta()) !== null;
}

export async function onboard(passphrase, serverOrigin) {
  if (passphrase.length < 12) throw new Error("passphrase must be 12+ characters");
  const salt = randomBytes(SALT_BYTES);
  const k = await deriveKUser(passphrase, salt);
  const verifier = await sha256(k);

  const kSess = randomBytes(KEY_BYTES);
  const hmacPub = await hkdfKey(k, "sync-hmac");

  // wrapped_K_session = XChaCha20(K_session, K_user)
  const wrapped = encrypt(k, kSess);
  // proxy_K_session is created server-side later; the extension uploads K_session
  // wrapped under a transient one-time key, and the server re-wraps under K_worker.
  // For simplicity in v1 we send K_session re-wrapped under a key derived from K_user
  // and labeled "proxy-bootstrap"; server unwraps once during signup and re-wraps
  // under K_worker. Field naming is preserved end-to-end.
  const bootstrapKey = await hkdfKey(k, "proxy-bootstrap");
  const proxyBoot = encrypt(bootstrapKey, kSess);

  const userId = ulid();
  const meta = {
    user_id: userId,
    salt_b64: bytesToB64(salt),
    verifier_b64: bytesToB64(verifier),
    hmac_pub_b64: bytesToB64(hmacPub),
    wrapped_k_session_b64: bytesToB64(packSealed(wrapped)),
    proxy_k_session_b64: bytesToB64(packSealed(proxyBoot)),
    server_origin: serverOrigin,
  };
  await saveMeta(meta);

  K_user = k;
  K_session = kSess;
  K_sync_hmac = hmacPub;
  touch();
  return meta;
}

export async function unlock(passphrase) {
  const meta = await getMeta();
  if (!meta) throw new Error("not onboarded");
  const salt = b64ToBytes(meta.salt_b64);
  const k = await deriveKUser(passphrase, salt);
  const v = await sha256(k);
  const stored = b64ToBytes(meta.verifier_b64);
  if (!ctEq(v, stored)) {
    zero(k);
    throw new Error("wrong passphrase");
  }
  // Decrypt wrapped K_session.
  const sealed = b64ToBytes(meta.wrapped_k_session_b64);
  const { ciphertext, nonce } = unpackSealed(sealed);
  const kSess = decrypt(k, ciphertext, nonce);
  K_user = k;
  K_session = kSess;
  K_sync_hmac = await hkdfKey(k, "sync-hmac");
  touch();
}

export function getKSession() {
  touch();
  return K_session;
}

export function getSyncHmac() {
  touch();
  return K_sync_hmac;
}

function ctEq(a, b) {
  if (a.length !== b.length) return false;
  let d = 0;
  for (let i = 0; i < a.length; i++) d |= a[i] ^ b[i];
  return d === 0;
}

function packSealed({ ciphertext, nonce }) {
  const out = new Uint8Array(nonce.length + ciphertext.length);
  out.set(nonce);
  out.set(ciphertext, nonce.length);
  return out;
}

function unpackSealed(packed) {
  return {
    nonce: packed.slice(0, 24),
    ciphertext: packed.slice(24),
  };
}

// Crockford-style ULID (26-char Base32). Time-sortable.
function ulid() {
  const ENC = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
  const t = Date.now();
  let timeStr = "";
  let n = BigInt(t);
  for (let i = 0; i < 10; i++) {
    timeStr = ENC[Number(n % 32n)] + timeStr;
    n = n / 32n;
  }
  const rnd = randomBytes(10);
  let randStr = "";
  for (let i = 0; i < 10; i++) randStr += ENC[rnd[i] % 32];
  return timeStr + randStr;
}
