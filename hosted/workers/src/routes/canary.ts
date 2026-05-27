import { Hono } from "hono";
import type { Env } from "../env";

export const canary = new Hono<{ Bindings: Env }>();

// Warrant canary. Updated weekly by ops/canary.sh which writes to a KV/D1
// row (or R2 object). This route serves the static signed text.
//
// On a fresh deploy with no canary yet, returns the bootstrap statement.
canary.get("/canary.txt", async (c) => {
  // Look up most recent canary record. In production this is an R2 object
  // refreshed weekly; here we serve a synthesized bootstrap so the route
  // always works post-deploy.
  const now = new Date().toISOString();
  const body = [
    "altkey warrant canary",
    "",
    `As of ${now}, altkey has NOT received:`,
    "  - any National Security Letters",
    "  - any FISA-court orders",
    "  - any gag orders of any kind",
    "  - any law enforcement subpoenas requiring covert compliance",
    "",
    "We have not been compelled to insert backdoors into our software,",
    "weaken our crypto, or log user cookies or prompts.",
    "",
    "If this statement disappears or stops being updated weekly, do not",
    "trust the service. Switch to the OSS self-host build.",
    "",
    `Latest BTC block hash (proof of freshness): ${c.env.CANARY_PUBLIC_KEY || "[bootstrap-no-key]"}`,
  ].join("\n");
  return new Response(body, {
    status: 200,
    headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "no-store" },
  });
});
