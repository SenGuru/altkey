// Service worker: watches provider cookies and pushes to /sync/upload.
//
// Cookies are read via chrome.cookies API (no host-page injection needed).
// Debounce 5s after the last cookie change in a burst.

import { uploadSession } from "./lib/sync.js";
import { isLocked, getMeta } from "./lib/state.js";

const PROVIDERS = {
  claude: {
    domains: ["claude.ai"],
    requiredCookies: ["sessionKey"],
  },
  chatgpt: {
    domains: ["chatgpt.com", ".chatgpt.com", ".openai.com"],
    requiredCookies: ["__Secure-next-auth.session-token"],
  },
  gemini: {
    domains: [".google.com", "gemini.google.com"],
    requiredCookies: ["__Secure-1PSID"],
  },
};

const DEBOUNCE_MS = 5000;
const debounceTimers = new Map(); // provider → timer

chrome.cookies.onChanged.addListener((info) => {
  const cookie = info.cookie;
  if (!cookie) return;
  const dom = (cookie.domain || "").replace(/^\./, "");
  for (const [provider, cfg] of Object.entries(PROVIDERS)) {
    if (cfg.domains.some((d) => dom.endsWith(d.replace(/^\./, "")))) {
      schedule(provider);
      return;
    }
  }
});

chrome.alarms.create("altkey-heartbeat", { periodInMinutes: 30 });
chrome.alarms.onAlarm.addListener((a) => {
  if (a.name === "altkey-heartbeat") {
    for (const p of Object.keys(PROVIDERS)) schedule(p);
  }
});

function schedule(provider) {
  if (debounceTimers.has(provider)) clearTimeout(debounceTimers.get(provider));
  const t = setTimeout(() => {
    debounceTimers.delete(provider);
    captureAndUpload(provider).catch((e) => console.warn("altkey sync", provider, e.message));
  }, DEBOUNCE_MS);
  debounceTimers.set(provider, t);
}

async function captureAndUpload(provider) {
  if (isLocked()) return;
  const meta = await getMeta();
  if (!meta) return;

  const cfg = PROVIDERS[provider];
  const all = [];
  for (const dom of cfg.domains) {
    const dnorm = dom.replace(/^\./, "");
    const cookies = await chrome.cookies.getAll({ domain: dnorm });
    for (const c of cookies) all.push(c);
  }

  // Dedupe by name@domain.
  const seen = new Set();
  const cookies = [];
  for (const c of all) {
    const k = `${c.name}@${c.domain}`;
    if (seen.has(k)) continue;
    seen.add(k);
    cookies.push({
      name: c.name,
      value: c.value,
      domain: c.domain,
      path: c.path,
      secure: c.secure,
      httpOnly: c.httpOnly,
      sameSite: c.sameSite,
      expirationDate: c.expirationDate ?? null,
    });
  }

  // Verify required cookies present; otherwise skip — user isn't logged in yet.
  const have = new Set(cookies.map((c) => c.name));
  if (!cfg.requiredCookies.every((rc) => have.has(rc))) return;

  const blob = {
    cookies,
    user_agent: navigator.userAgent,
  };
  await uploadSession(provider, blob);
}

// Listen for popup-driven manual refresh.
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type === "altkey:refresh") {
    (async () => {
      try {
        for (const p of Object.keys(PROVIDERS)) await captureAndUpload(p);
        sendResponse({ ok: true });
      } catch (e) {
        sendResponse({ ok: false, error: e.message });
      }
    })();
    return true;
  }
  if (msg?.type === "altkey:status") {
    (async () => {
      const meta = await getMeta();
      sendResponse({
        locked: isLocked(),
        onboarded: !!meta,
        user_id: meta?.user_id,
        server_origin: meta?.server_origin,
      });
    })();
    return true;
  }
});
