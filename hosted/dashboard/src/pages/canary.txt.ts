import type { APIRoute } from "astro";

// The dashboard proxies the canary so it lives at altkey.app/canary.txt
// alongside the marketing pages. Worker route serves the signed body.
export const GET: APIRoute = async () => {
  const API_BASE = import.meta.env.PUBLIC_API_BASE || "https://api.altkey.app";
  try {
    const r = await fetch(`${API_BASE}/canary.txt`);
    const body = await r.text();
    return new Response(body, {
      status: r.status,
      headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "no-store" },
    });
  } catch (e) {
    return new Response(`canary unreachable: ${e instanceof Error ? e.message : String(e)}`, {
      status: 502,
      headers: { "content-type": "text/plain; charset=utf-8" },
    });
  }
};
