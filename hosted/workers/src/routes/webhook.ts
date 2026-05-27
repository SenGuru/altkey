import { Hono } from "hono";
import type { Env } from "../env";
import { verifyPolarWebhook, parsePolarEvent } from "../billing/polar";
import { verifyNowpaymentsIpn, parseNowpaymentsEvent } from "../billing/nowpayments";
import { insertPayment, setUserPlan } from "../db";
import { log } from "../log";

export const webhook = new Hono<{ Bindings: Env }>();

webhook.post("/wh/polar", async (c) => {
  const sig = c.req.header("polar-signature") ?? "";
  const body = await c.req.text();
  if (!(await verifyPolarWebhook(c.env.POLAR_WEBHOOK_SECRET, body, sig))) {
    return c.json({ error: "invalid signature" }, 401);
  }
  const evt = parsePolarEvent(body);
  if (!evt) return c.json({ ok: true });
  const ins = await insertPayment(c.env.DB, {
    id: `polar_${evt.external_id}`,
    user_id: evt.user_id,
    processor: "polar",
    external_id: evt.external_id,
    status: evt.status,
    amount_cents: evt.amount_cents,
  });
  if (ins.inserted && evt.status === "succeeded") {
    await setUserPlan(c.env.DB, evt.user_id, "pro");
  }
  log("info", "webhook.polar", { evt: evt.kind, user: evt.user_id, status: evt.status });
  return c.json({ ok: true });
});

webhook.post("/wh/nowpayments", async (c) => {
  const sig = c.req.header("x-nowpayments-sig") ?? "";
  const body = await c.req.text();
  if (!(await verifyNowpaymentsIpn(c.env.NOWPAYMENTS_IPN_SECRET, body, sig))) {
    return c.json({ error: "invalid signature" }, 401);
  }
  const evt = parseNowpaymentsEvent(body);
  if (!evt) return c.json({ ok: true });
  const ins = await insertPayment(c.env.DB, {
    id: `nowp_${evt.external_id}`,
    user_id: evt.user_id,
    processor: "nowpayments",
    external_id: evt.external_id,
    status: evt.status,
    amount_cents: evt.amount_cents,
  });
  if (ins.inserted && evt.status === "succeeded") {
    await setUserPlan(c.env.DB, evt.user_id, "pro");
  }
  log("info", "webhook.nowpayments", { evt: evt.status, user: evt.user_id });
  return c.json({ ok: true });
});
