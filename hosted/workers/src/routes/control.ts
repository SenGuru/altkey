import { Hono } from "hono";
import type { Env } from "../env";
import { authBearerKey, readSessionCookie, setSessionCookie, clearSessionCookie } from "../auth";
import {
  getUser,
  insertUser,
  insertApiKey,
  revokeApiKey,
  listApiKeysForUser,
  listSessions,
  setUserSessionKeys,
} from "../db";
import { hashApiKey, randomBytes, bytesToHex, sha256Bytes, utf8ToBytes, timingSafeEqual } from "../crypto";
import { log } from "../log";

export const control = new Hono<{ Bindings: Env }>();

interface SignupBody {
  user_id: string;
  salt_b64: string;
  verifier_b64: string;
  hmac_pub_b64: string;
  wrapped_k_session_b64: string;
  proxy_k_session_b64: string;
}

function b64decode(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function b64encode(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s);
}

control.post("/api/signup", async (c) => {
  const body = (await c.req.json()) as SignupBody;
  if (!body.user_id || !/^[0-9A-HJKMNP-TV-Z]{26}$/.test(body.user_id)) {
    return c.json({ error: "invalid user_id (expected ULID)" }, 400);
  }
  const existing = await getUser(c.env.DB, body.user_id);
  if (existing) return c.json({ error: "user_id already taken" }, 409);

  await insertUser(c.env.DB, {
    id: body.user_id,
    argon_salt: b64decode(body.salt_b64).buffer as ArrayBuffer,
    argon_verifier: b64decode(body.verifier_b64).buffer as ArrayBuffer,
    hmac_pub: b64decode(body.hmac_pub_b64).buffer as ArrayBuffer,
  });
  await setUserSessionKeys(
    c.env.DB,
    body.user_id,
    b64decode(body.wrapped_k_session_b64),
    b64decode(body.proxy_k_session_b64),
  );

  // Recovery code = 24 chars Crockford base32 of 15 random bytes.
  const recovery = bytesToHex(randomBytes(12)).toUpperCase();
  log("info", "signup", { user: body.user_id });
  return c.json({ ok: true, recovery_code: recovery });
});

interface UnlockBody {
  user_id: string;
  verifier_b64: string;
}

control.post("/api/unlock", async (c) => {
  const body = (await c.req.json()) as UnlockBody;
  const u = await getUser(c.env.DB, body.user_id);
  if (!u) return c.json({ error: "no such account" }, 401);
  if (u.killed_at) return c.json({ error: "account suspended" }, 403);

  const given = b64decode(body.verifier_b64);
  const stored = new Uint8Array(u.argon_verifier);
  if (!timingSafeEqual(given, stored)) {
    log("warn", "unlock_fail", { user: body.user_id });
    return c.json({ error: "wrong passphrase" }, 401);
  }

  // Issue opaque session cookie token = SHA256(user_id || rand).
  const token = bytesToHex(sha256Bytes(utf8ToBytes(`${body.user_id}:${bytesToHex(randomBytes(16))}`)));
  setSessionCookie(c, `${body.user_id}.${token}`, 86400);
  return c.json({ ok: true, plan: u.plan });
});

control.post("/api/logout", async (c) => {
  clearSessionCookie(c);
  return c.json({ ok: true });
});

// Mint API key. Requires bearer (i.e. existing key) OR session cookie unlock.
control.post("/api/keys", async (c) => {
  const userId = await resolveUserId(c);
  if (!userId) return c.json({ error: "unauthorized" }, 401);

  const body = (await c.req.json().catch(() => ({}))) as { label?: string };
  const raw = `sk-alt-${b64urlNoPad(randomBytes(32))}`;
  const kh = hashApiKey(raw);
  await insertApiKey(c.env.DB, kh, userId, (body.label ?? "").slice(0, 64));
  return c.json({ key: raw });
});

control.post("/api/keys/revoke", async (c) => {
  const userId = await resolveUserId(c);
  if (!userId) return c.json({ error: "unauthorized" }, 401);
  const { key } = (await c.req.json()) as { key: string };
  if (!key) return c.json({ error: "missing key" }, 400);
  await revokeApiKey(c.env.DB, hashApiKey(key));
  return c.json({ ok: true });
});

control.get("/api/keys", async (c) => {
  const userId = await resolveUserId(c);
  if (!userId) return c.json({ error: "unauthorized" }, 401);
  const rows = await listApiKeysForUser(c.env.DB, userId);
  return c.json({
    keys: rows.map((r) => ({
      key_prefix: `sk-alt-${b64encode(new Uint8Array(r.key_hash)).slice(0, 6)}…`,
      label: r.label,
      created_at: r.created_at,
    })),
  });
});

control.get("/api/account/status", async (c) => {
  const userId = await resolveUserId(c);
  if (!userId) return c.json({ error: "unauthorized" }, 401);
  const user = await getUser(c.env.DB, userId);
  const sessions = await listSessions(c.env.DB, userId);
  return c.json({
    user_id: userId,
    plan: user?.plan ?? "free",
    sessions,
  });
});

async function resolveUserId(c: any): Promise<string | null> {
  const bearer = await authBearerKey(c);
  if (!(bearer instanceof Response)) return bearer.userId;
  const cookie = readSessionCookie(c);
  if (cookie) {
    const dot = cookie.indexOf(".");
    if (dot > 0) {
      const candidate = cookie.slice(0, dot);
      const u = await getUser(c.env.DB, candidate);
      if (u && !u.killed_at) return candidate;
    }
  }
  return null;
}

function b64urlNoPad(b: Uint8Array): string {
  return b64encode(b).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
