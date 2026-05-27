import { Hono } from "hono";
import type { Env } from "../env";
import { authBearerKey } from "../auth";
import { checkDailyQuota } from "../quota";
import { forModel, listModels, type SessionBlob } from "../providers/index";
import { getSession, getUser, markSessionStale, recordUsage } from "../db";
import { decrypt, deriveKWorker, unwrapKSessionFromProxy, zero } from "../crypto";
import { openaiChunk, openaiCompletion } from "../utils/openai";
import { sseStream, SSE_HEADERS } from "../utils/sse";
import { log } from "../log";

export const proxy = new Hono<{ Bindings: Env }>();

proxy.get("/v1/models", async (c) => {
  const a = await authBearerKey(c);
  if (a instanceof Response) return a;
  return c.json({ object: "list", data: listModels() });
});

proxy.post("/v1/chat/completions", async (c) => {
  const a = await authBearerKey(c);
  if (a instanceof Response) return a;
  const { userId, plan } = a;

  const body = (await c.req.json()) as { model?: string; messages?: unknown; stream?: boolean };
  const model = body.model ?? "claude-sonnet-4-5";
  const mod = forModel(model);
  if (!mod) {
    return c.json({ error: { message: `unknown model: ${model}`, type: "model_not_found" } }, 400);
  }

  const provider = mod.PROVIDER_ID;
  const decision = await checkDailyQuota(c.env, userId, plan, provider);
  if (!decision.allowed) {
    const reason = decision.reason ?? "quota";
    return c.json(
      { error: { message: `quota.${reason}`, type: "quota.exceeded" } },
      reason === "hard_kill" ? 403 : 429,
    );
  }

  const sessionRow = await getSession(c.env.DB, userId, provider);
  if (!sessionRow) {
    return c.json(
      { error: { message: `${provider} not connected`, type: "session.missing" } },
      400,
    );
  }
  const user = await getUser(c.env.DB, userId);
  if (!user?.proxy_k_session) {
    return c.json(
      { error: { message: "session keys not initialized — sync via extension", type: "session.missing" } },
      400,
    );
  }

  // Derive K_worker, unwrap K_session, decrypt session blob — all in RAM.
  const kWorker = deriveKWorker(c.env.CF_SECRET, userId);
  let kSession: Uint8Array | null = null;
  let blob: SessionBlob;
  try {
    const proxyPack = new Uint8Array(user.proxy_k_session);
    if (proxyPack.length < 24 + 16) throw new Error("proxy_k_session blob too short");
    const proxyNonce = proxyPack.slice(0, 24);
    const proxyCt = proxyPack.slice(24);
    kSession = unwrapKSessionFromProxy(proxyCt, proxyNonce, kWorker);

    const sessNonce = new Uint8Array(sessionRow.nonce);
    const sessCt = new Uint8Array(sessionRow.ciphertext);
    const blobBytes = decrypt(kSession, sessCt, sessNonce);
    blob = JSON.parse(new TextDecoder().decode(blobBytes)) as SessionBlob;
  } catch (e) {
    log("error", "crypto.decrypt_failed", { user: userId, provider });
    return c.json({ error: { message: "internal decrypt failure", type: "crypto.decrypt_failed" } }, 500);
  } finally {
    zero(kWorker);
  }

  const wantStream = body.stream === true;
  const messages = (body.messages as Array<{ role: string; content: unknown }> | undefined) ?? [];
  const chatReq = { model, messages: messages as never };

  let totalBytes = 0;

  async function* run() {
    try {
      for await (const evt of mod!.stream(chatReq as never, blob)) {
        totalBytes += evt.delta.length;
        yield evt;
      }
    } finally {
      if (kSession) zero(kSession);
    }
  }

  if (wantStream) {
    const gen = (async function* () {
      try {
        for await (const evt of run()) {
          yield openaiChunk(model, evt.delta);
        }
        yield openaiChunk(model, "", "stop");
        yield "DONE" as const;
      } finally {
        await recordUsage(c.env.DB, userId, provider, model, totalBytes).catch(() => {});
      }
    })();
    return new Response(sseStream(gen), { status: 200, headers: SSE_HEADERS });
  }

  const parts: string[] = [];
  try {
    for await (const evt of run()) parts.push(evt.delta);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes("expired") || msg.includes("401") || msg.includes("403")) {
      await markSessionStale(c.env.DB, userId, provider).catch(() => {});
    }
    return c.json({ error: { message: msg, type: "upstream_error" } }, 502);
  }
  await recordUsage(c.env.DB, userId, provider, model, totalBytes).catch(() => {});
  return c.json(openaiCompletion(model, parts.join("")));
});
