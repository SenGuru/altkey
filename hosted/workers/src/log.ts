// Scrubbing logger. Never log cookies, secrets, or completion content.

const COOKIE_NAMES = new Set([
  "sessionkey",
  "__secure-next-auth.session-token",
  "__secure-1psid",
  "__secure-1psidts",
  "__secure-1psidcc",
  "_cfuvid",
  "cf_clearance",
  "_dd_s",
  "oai-did",
]);

const SK_RE = /sk-alt-[A-Za-z0-9_\-]{16,}/g;
const LONG_B64_RE = /[A-Za-z0-9+/=]{120,}/g;

function scrub(v: unknown): unknown {
  if (typeof v === "string") {
    let s = v.replace(SK_RE, "sk-alt-[REDACTED]").replace(LONG_B64_RE, "[B64-REDACTED]");
    return s;
  }
  if (Array.isArray(v)) return v.map(scrub);
  if (v && typeof v === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, val] of Object.entries(v)) {
      if (COOKIE_NAMES.has(k.toLowerCase())) {
        out[k] = "[REDACTED]";
      } else {
        out[k] = scrub(val);
      }
    }
    return out;
  }
  return v;
}

export function log(level: "info" | "warn" | "error", event: string, fields?: Record<string, unknown>): void {
  const entry = { t: Date.now(), level, event, ...(fields ? (scrub(fields) as Record<string, unknown>) : {}) };
  if (level === "error") console.error(JSON.stringify(entry));
  else if (level === "warn") console.warn(JSON.stringify(entry));
  else console.log(JSON.stringify(entry));
}
