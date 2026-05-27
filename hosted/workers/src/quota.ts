// Quota + abuse caps. See spec §6.3.
//
// Per-day caps are read from the usage table (cheap aggregation).
// Per-key concurrency would need Durable Objects in prod; this module
// returns the would-be limits and leaves rate-limiting at the edge to
// Cloudflare's Rate Limiting rules + Turnstile for signup.

import type { Env } from "./env";
import { usageToday } from "./db";

export const FREE_DAILY_MESSAGES = 50;
export const PRO_DAILY_MESSAGES = Infinity;
export const PROVIDER_SOFT_CAP = 1000;
export const PROVIDER_HARD_KILL = 5000;

export interface QuotaDecision {
  allowed: boolean;
  reason?: "free_cap" | "soft_cap" | "hard_kill";
  remaining?: number;
}

export async function checkDailyQuota(
  env: Env,
  userId: string,
  plan: string,
  provider: string,
): Promise<QuotaDecision> {
  const totalToday = await usageToday(env.DB, userId);
  if (plan === "free" && totalToday >= FREE_DAILY_MESSAGES) {
    return { allowed: false, reason: "free_cap" };
  }
  const perProvider = await usageToday(env.DB, userId, provider);
  if (perProvider >= PROVIDER_HARD_KILL) {
    return { allowed: false, reason: "hard_kill" };
  }
  if (perProvider >= PROVIDER_SOFT_CAP) {
    // Soft cap: we let the request through with a header warning. Hard
    // throttling happens at the edge via CF Rate Limiting rules.
    return { allowed: true, reason: "soft_cap", remaining: PROVIDER_HARD_KILL - perProvider };
  }
  return { allowed: true, remaining: plan === "free" ? FREE_DAILY_MESSAGES - totalToday : undefined };
}
