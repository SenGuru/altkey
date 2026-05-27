// NOWPayments crypto integration. IPN signature is HMAC-SHA512 in
// header `x-nowpayments-sig` over alphabetically-sorted JSON body.

import { hmac } from "@noble/hashes/hmac";
import { sha512 } from "@noble/hashes/sha512";
import { utf8ToBytes, bytesToHex, timingSafeEqual } from "../crypto";

export async function verifyNowpaymentsIpn(secret: string, body: string, sig: string): Promise<boolean> {
  if (!secret || !sig) return false;
  let data: any;
  try {
    data = JSON.parse(body);
  } catch {
    return false;
  }
  const sorted = JSON.stringify(sortKeys(data));
  const expected = bytesToHex(hmac(sha512, utf8ToBytes(secret), utf8ToBytes(sorted)));
  return timingSafeEqual(utf8ToBytes(expected), utf8ToBytes(sig.toLowerCase()));
}

function sortKeys(o: any): any {
  if (Array.isArray(o)) return o.map(sortKeys);
  if (o && typeof o === "object") {
    const out: Record<string, unknown> = {};
    for (const k of Object.keys(o).sort()) out[k] = sortKeys(o[k]);
    return out;
  }
  return o;
}

export interface NowpaymentsEvent {
  user_id: string;
  external_id: string;
  status: "succeeded" | "failed" | "pending" | "refunded";
  amount_cents: number | null;
}

export function parseNowpaymentsEvent(body: string): NowpaymentsEvent | null {
  let data: any;
  try {
    data = JSON.parse(body);
  } catch {
    return null;
  }
  const userId = data?.order_description as string | undefined; // we put user_id here on invoice creation
  const externalId = data?.payment_id ? String(data.payment_id) : null;
  if (!userId || !externalId) return null;

  const status: NowpaymentsEvent["status"] =
    data?.payment_status === "finished" || data?.payment_status === "confirmed"
      ? "succeeded"
      : data?.payment_status === "failed" || data?.payment_status === "expired"
        ? "failed"
        : data?.payment_status === "refunded"
          ? "refunded"
          : "pending";

  const amountCents =
    typeof data?.price_amount === "number" && data?.price_currency === "usd"
      ? Math.round(data.price_amount * 100)
      : null;

  return { user_id: userId, external_id: externalId, status, amount_cents: amountCents };
}

export async function createInvoice(
  apiKey: string,
  userId: string,
  priceUsd: number,
  successUrl: string,
): Promise<{ url: string }> {
  const r = await fetch("https://api.nowpayments.io/v1/invoice", {
    method: "POST",
    headers: { "x-api-key": apiKey, "Content-Type": "application/json" },
    body: JSON.stringify({
      price_amount: priceUsd,
      price_currency: "usd",
      order_description: userId,
      success_url: successUrl,
      ipn_callback_url: `${successUrl.replace(/\/[^/]*$/, "")}/wh/nowpayments`,
    }),
  });
  if (!r.ok) throw new Error(`nowpayments invoice ${r.status}: ${await r.text()}`);
  const j = (await r.json()) as { invoice_url: string };
  return { url: j.invoice_url };
}
