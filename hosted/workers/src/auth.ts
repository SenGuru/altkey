import type { Context } from "hono";
import type { Env } from "./env";
import { hashApiKey } from "./crypto";
import { findApiKeyOwner, getUser } from "./db";

export interface AuthCtx {
  userId: string;
  plan: string;
}

export async function authBearerKey(c: Context<{ Bindings: Env }>): Promise<AuthCtx | Response> {
  const hdr = c.req.header("authorization") ?? "";
  if (!hdr.toLowerCase().startsWith("bearer ")) {
    return new Response(JSON.stringify({ error: { message: "missing bearer token", type: "auth.invalid_key" } }), {
      status: 401,
      headers: { "content-type": "application/json" },
    });
  }
  const key = hdr.slice(7).trim();
  if (!key.startsWith("sk-alt-")) {
    return new Response(JSON.stringify({ error: { message: "invalid api key format", type: "auth.invalid_key" } }), {
      status: 401,
      headers: { "content-type": "application/json" },
    });
  }
  const kh = hashApiKey(key);
  const owner = await findApiKeyOwner(c.env.DB, kh);
  if (!owner) {
    return new Response(JSON.stringify({ error: { message: "invalid api key", type: "auth.invalid_key" } }), {
      status: 401,
      headers: { "content-type": "application/json" },
    });
  }
  const user = await getUser(c.env.DB, owner.user_id);
  if (!user) {
    return new Response(JSON.stringify({ error: { message: "account not found", type: "auth.invalid_key" } }), {
      status: 401,
      headers: { "content-type": "application/json" },
    });
  }
  if (user.killed_at) {
    return new Response(JSON.stringify({ error: { message: "account suspended", type: "auth.killed" } }), {
      status: 403,
      headers: { "content-type": "application/json" },
    });
  }
  return { userId: user.id, plan: user.plan };
}

// Dashboard session cookie helpers. The cookie value is opaque to the server's
// crypto layer — it only proves the user has unlocked recently.
const SESSION_COOKIE = "altkey_session";

export function setSessionCookie(c: Context, token: string, maxAgeSeconds = 86400): void {
  c.header(
    "set-cookie",
    `${SESSION_COOKIE}=${token}; Max-Age=${maxAgeSeconds}; Path=/; HttpOnly; Secure; SameSite=Strict`,
  );
}

export function clearSessionCookie(c: Context): void {
  c.header(
    "set-cookie",
    `${SESSION_COOKIE}=; Max-Age=0; Path=/; HttpOnly; Secure; SameSite=Strict`,
  );
}

export function readSessionCookie(c: Context): string | null {
  const raw = c.req.header("cookie");
  if (!raw) return null;
  for (const part of raw.split(";")) {
    const [k, v] = part.trim().split("=");
    if (k === SESSION_COOKIE && v) return v;
  }
  return null;
}
