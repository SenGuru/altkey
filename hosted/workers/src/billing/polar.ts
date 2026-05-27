// Polar.sh billing integration. Webhook signature is HMAC-SHA256(body, secret)
// hex-encoded, sent in header `polar-signature`.

import { hmacSign, utf8ToBytes, bytesToHex, timingSafeEqual } from "../crypto";

export async function verifyPolarWebhook(secret: string, body: string, sig: string): Promise<boolean> {
  if (!secret || !sig) return false;
  const expected = bytesToHex(hmacSign(utf8ToBytes(secret), utf8ToBytes(body)));
  return timingSafeEqual(utf8ToBytes(expected), utf8ToBytes(sig.toLowerCase()));
}

export interface PolarEvent {
  kind: string;
  user_id: string;
  external_id: string;
  status: "succeeded" | "failed" | "pending" | "refunded" | "canceled";
  amount_cents: number | null;
}

export function parsePolarEvent(body: string): PolarEvent | null {
  let data: any;
  try {
    data = JSON.parse(body);
  } catch {
    return null;
  }
  const kind = data?.type as string | undefined;
  const obj = data?.data;
  if (!kind || !obj) return null;

  const userId = (obj.metadata?.user_id ?? obj.customer?.metadata?.user_id) as string | undefined;
  if (!userId) return null;

  let status: PolarEvent["status"] = "pending";
  if (kind === "subscription.created" || kind === "subscription.active" || kind === "checkout.completed") {
    status = "succeeded";
  } else if (kind === "subscription.canceled") {
    status = "canceled";
  } else if (kind === "subscription.refunded" || kind === "refund.created") {
    status = "refunded";
  } else if (kind === "subscription.failed" || kind === "checkout.failed") {
    status = "failed";
  }

  const externalId = (obj.id ?? obj.subscription_id ?? obj.checkout_id) as string | undefined;
  if (!externalId) return null;

  const amount = typeof obj.amount === "number" ? obj.amount : null;
  return {
    kind,
    user_id: userId,
    external_id: externalId,
    status,
    amount_cents: amount,
  };
}

// Server-side checkout creation. Called from /api/billing/checkout in control.ts
// (not wired in this scaffold; add when product IDs are set in Polar dashboard).
export async function createCheckout(
  apiKey: string,
  productId: string,
  userId: string,
  successUrl: string,
): Promise<{ url: string }> {
  const r = await fetch("https://api.polar.sh/api/v1/checkouts/", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      product_id: productId,
      success_url: successUrl,
      metadata: { user_id: userId },
    }),
  });
  if (!r.ok) throw new Error(`polar checkout ${r.status}: ${await r.text()}`);
  const j = (await r.json()) as { url: string };
  return { url: j.url };
}
