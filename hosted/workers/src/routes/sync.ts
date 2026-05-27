import { Hono } from "hono";
import type { Env } from "../env";
import { getUser, upsertSession, listSessions } from "../db";
import { hmacVerify, utf8ToBytes } from "../crypto";
import { log } from "../log";

export const sync = new Hono<{ Bindings: Env }>();

interface UploadBody {
  user_id: string;
  provider: string;
  ciphertext_b64: string;
  nonce_b64: string;
  captured_at: number;
  signature_b64: string;
}

function b64decode(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

const ALLOWED_PROVIDERS = new Set(["claude", "chatgpt", "gemini"]);

sync.post("/sync/upload", async (c) => {
  const body = (await c.req.json()) as UploadBody;
  if (!body.user_id || !ALLOWED_PROVIDERS.has(body.provider)) {
    return c.json({ error: "invalid request" }, 400);
  }
  const user = await getUser(c.env.DB, body.user_id);
  if (!user) return c.json({ error: "no such account" }, 404);
  if (user.killed_at) return c.json({ error: "account suspended" }, 403);

  const ciphertext = b64decode(body.ciphertext_b64);
  const nonce = b64decode(body.nonce_b64);
  const sig = b64decode(body.signature_b64);
  const hmacKey = new Uint8Array(user.hmac_pub);

  // Signed payload = ciphertext || nonce || captured_at(big-endian 8 bytes) || provider.
  const ts = new ArrayBuffer(8);
  new DataView(ts).setBigUint64(0, BigInt(body.captured_at), false);
  const signed = concat([ciphertext, nonce, new Uint8Array(ts), utf8ToBytes(body.provider)]);
  if (!hmacVerify(hmacKey, signed, sig)) {
    log("warn", "sync.hmac_fail", { user: body.user_id, provider: body.provider });
    return c.json({ error: "invalid signature" }, 401);
  }

  await upsertSession(c.env.DB, {
    user_id: body.user_id,
    provider: body.provider,
    ciphertext,
    nonce,
    updated_at: body.captured_at,
  });
  return c.json({ ok: true });
});

sync.get("/sync/status", async (c) => {
  const userId = c.req.query("user_id");
  if (!userId) return c.json({ error: "missing user_id" }, 400);
  const sessions = await listSessions(c.env.DB, userId);
  return c.json({ sessions });
});

function concat(arrs: Uint8Array[]): Uint8Array {
  const total = arrs.reduce((s, a) => s + a.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const a of arrs) {
    out.set(a, o);
    o += a.length;
  }
  return out;
}
