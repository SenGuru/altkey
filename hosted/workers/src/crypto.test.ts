import { describe, it, expect } from "vitest";
import {
  encrypt,
  decrypt,
  hkdfKey,
  deriveKWorker,
  wrapKSessionForProxy,
  unwrapKSessionFromProxy,
  hmacSign,
  hmacVerify,
  hashApiKey,
  randomBytes,
  pack,
  unpack,
  bytesToHex,
  utf8ToBytes,
} from "./crypto";

describe("XChaCha20-Poly1305", () => {
  it("roundtrips arbitrary bytes", () => {
    const key = randomBytes(32);
    const pt = utf8ToBytes("hello world");
    const { ciphertext, nonce } = encrypt(key, pt);
    const dec = decrypt(key, ciphertext, nonce);
    expect(bytesToHex(dec)).toBe(bytesToHex(pt));
  });

  it("rejects wrong key", () => {
    const k1 = randomBytes(32);
    const k2 = randomBytes(32);
    const { ciphertext, nonce } = encrypt(k1, utf8ToBytes("secret"));
    expect(() => decrypt(k2, ciphertext, nonce)).toThrow();
  });

  it("rejects tampered ciphertext", () => {
    const key = randomBytes(32);
    const { ciphertext, nonce } = encrypt(key, utf8ToBytes("secret"));
    const tampered = new Uint8Array(ciphertext);
    tampered[0] = tampered[0]! ^ 1;
    expect(() => decrypt(key, tampered, nonce)).toThrow();
  });

  it("nonces are unique across calls", () => {
    const key = randomBytes(32);
    const a = encrypt(key, utf8ToBytes("x"));
    const b = encrypt(key, utf8ToBytes("x"));
    expect(bytesToHex(a.nonce)).not.toBe(bytesToHex(b.nonce));
  });
});

describe("HKDF + K_worker derivation", () => {
  it("hkdfKey is deterministic for same inputs", () => {
    const ikm = utf8ToBytes("input-keying-material");
    const a = hkdfKey(ikm, "context-a");
    const b = hkdfKey(ikm, "context-a");
    expect(bytesToHex(a)).toBe(bytesToHex(b));
  });

  it("hkdfKey differs across contexts", () => {
    const ikm = utf8ToBytes("input-keying-material");
    const a = hkdfKey(ikm, "context-a");
    const b = hkdfKey(ikm, "context-b");
    expect(bytesToHex(a)).not.toBe(bytesToHex(b));
  });

  it("K_worker differs across users", () => {
    const secret = bytesToHex(randomBytes(32));
    const ka = deriveKWorker(secret, "user-A");
    const kb = deriveKWorker(secret, "user-B");
    expect(bytesToHex(ka)).not.toBe(bytesToHex(kb));
  });

  it("K_worker depends on CF_SECRET", () => {
    const a = deriveKWorker(bytesToHex(randomBytes(32)), "user-X");
    const b = deriveKWorker(bytesToHex(randomBytes(32)), "user-X");
    expect(bytesToHex(a)).not.toBe(bytesToHex(b));
  });
});

describe("K_session wrap/unwrap for proxy use", () => {
  it("roundtrip", () => {
    const kWorker = randomBytes(32);
    const kSession = randomBytes(32);
    const { ciphertext, nonce } = wrapKSessionForProxy(kSession, kWorker);
    const out = unwrapKSessionFromProxy(ciphertext, nonce, kWorker);
    expect(bytesToHex(out)).toBe(bytesToHex(kSession));
  });
});

describe("HMAC", () => {
  it("verifies known signature", () => {
    const key = randomBytes(32);
    const msg = utf8ToBytes("payload");
    const sig = hmacSign(key, msg);
    expect(hmacVerify(key, msg, sig)).toBe(true);
  });

  it("rejects wrong key", () => {
    const sig = hmacSign(randomBytes(32), utf8ToBytes("payload"));
    expect(hmacVerify(randomBytes(32), utf8ToBytes("payload"), sig)).toBe(false);
  });

  it("rejects tampered message", () => {
    const key = randomBytes(32);
    const sig = hmacSign(key, utf8ToBytes("payload"));
    expect(hmacVerify(key, utf8ToBytes("payloaD"), sig)).toBe(false);
  });
});

describe("API key hashing", () => {
  it("is deterministic", () => {
    const a = hashApiKey("sk-alt-test");
    const b = hashApiKey("sk-alt-test");
    expect(bytesToHex(a)).toBe(bytesToHex(b));
  });

  it("differs across keys", () => {
    const a = hashApiKey("sk-alt-A");
    const b = hashApiKey("sk-alt-B");
    expect(bytesToHex(a)).not.toBe(bytesToHex(b));
  });
});

describe("pack/unpack", () => {
  it("roundtrips ciphertext + nonce", () => {
    const key = randomBytes(32);
    const { ciphertext, nonce } = encrypt(key, utf8ToBytes("payload"));
    const packed = pack(ciphertext, nonce);
    const out = unpack(packed);
    expect(bytesToHex(out.nonce)).toBe(bytesToHex(nonce));
    expect(bytesToHex(out.ciphertext)).toBe(bytesToHex(ciphertext));
  });
});

// ──────────────────────────────────────────────────────────────────────────
// THREAT MODEL REGRESSION — executable form of spec §3.2.
// Adversary has D1 dump (ciphertext + proxy_k_session ciphertext + user_id)
// but does NOT have CF_SECRET. Decryption must fail.
// ──────────────────────────────────────────────────────────────────────────
describe("THREAT MODEL: DB-dump without CF_SECRET cannot decrypt", () => {
  function setupUserAndStore() {
    const cfSecret = bytesToHex(randomBytes(32));
    const userId = "user-XYZ";
    const kWorker = deriveKWorker(cfSecret, userId);
    const kSession = randomBytes(32);
    const wrapped = wrapKSessionForProxy(kSession, kWorker);

    // Session blob (would normally be the user's encrypted cookies).
    const sessionBlob = utf8ToBytes(JSON.stringify({ cookies: ["secret"] }));
    const stored = encrypt(kSession, sessionBlob);

    return { cfSecret, userId, wrapped, stored };
  }

  it("with correct CF_SECRET, decryption succeeds", () => {
    const { cfSecret, userId, wrapped, stored } = setupUserAndStore();
    const kWorker = deriveKWorker(cfSecret, userId);
    const kSession = unwrapKSessionFromProxy(wrapped.ciphertext, wrapped.nonce, kWorker);
    const blob = decrypt(kSession, stored.ciphertext, stored.nonce);
    expect(new TextDecoder().decode(blob)).toContain("secret");
  });

  it("with wrong CF_SECRET, K_session unwrap fails", () => {
    const { userId, wrapped } = setupUserAndStore();
    const wrongSecret = bytesToHex(randomBytes(32));
    const kWorkerWrong = deriveKWorker(wrongSecret, userId);
    expect(() =>
      unwrapKSessionFromProxy(wrapped.ciphertext, wrapped.nonce, kWorkerWrong),
    ).toThrow();
  });

  it("with no CF_SECRET (DB-only adversary), session blob cannot be decrypted", () => {
    const { stored } = setupUserAndStore();
    // Attacker tries a guessed K_session.
    const guessedKey = randomBytes(32);
    expect(() => decrypt(guessedKey, stored.ciphertext, stored.nonce)).toThrow();
  });

  it("with right CF_SECRET but wrong user_id, K_worker is wrong", () => {
    const { cfSecret, userId, wrapped } = setupUserAndStore();
    const kWorkerOtherUser = deriveKWorker(cfSecret, userId + "-attacker-flip");
    expect(() =>
      unwrapKSessionFromProxy(wrapped.ciphertext, wrapped.nonce, kWorkerOtherUser),
    ).toThrow();
  });
});
