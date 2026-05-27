import { cookieHeader, flattenContent, type ChatRequest, type SessionBlob } from "./index";

export const NAME = "gemini";
export const PROVIDER_ID = "gemini";
export const MODELS = ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash", "gemini-1.5-pro"];

const BASE = "https://gemini.google.com";
const DOMAINS = ["google.com"];
const SNLM_RE = /"SNlM0e":"([^"]+)"/;

const MODEL_HEADER: Record<string, [string, number]> = {
  "gemini-2.5-pro": ["c_a065d44e", 1],
  "gemini-2.5-flash": ["c_a065d44e", 0],
  "gemini-2.0-flash": ["c_835d8b8c", 0],
  "gemini-1.5-pro": ["c_70f59a40", 1],
};

function headers(session: SessionBlob, contentType?: string): HeadersInit {
  const h: Record<string, string> = {
    "User-Agent": session.user_agent || "Mozilla/5.0",
    Accept: "*/*",
    "Accept-Language": "en-US,en;q=0.9",
    Origin: BASE,
    Referer: `${BASE}/app`,
    "X-Same-Domain": "1",
    Cookie: cookieHeader(session, DOMAINS),
  };
  if (contentType) h["Content-Type"] = contentType;
  return h;
}

async function snlm0e(session: SessionBlob): Promise<string> {
  const r = await fetch(`${BASE}/app`, { headers: headers(session) });
  if (!r.ok) throw new Error(`gemini /app ${r.status} — re-connect`);
  const html = await r.text();
  const m = SNLM_RE.exec(html);
  if (!m) throw new Error("gemini session expired — re-connect");
  return m[1]!;
}

function flatten(messages: ChatRequest["messages"]): string {
  return messages
    .map((m) => {
      const tag = m.role === "user" ? "User" : m.role === "assistant" ? "Assistant" : m.role === "system" ? "System" : m.role;
      return `${tag}: ${flattenContent(m.content)}`;
    })
    .join("\n\n");
}

export async function* stream(req: ChatRequest, session: SessionBlob): AsyncIterable<{ delta: string }> {
  const snlm = await snlm0e(session);
  const prompt = flatten(req.messages);
  const modelHdr = MODEL_HEADER[req.model] ?? MODEL_HEADER["gemini-2.5-flash"]!;
  const reqId = String(Math.floor(Math.random() * 1_000_000));

  const inner: unknown[] = [[prompt], null, [null, null, null, [], null, null, "", 0, 0, 0, [], 0, 0, null, 0, 0, [], 0, 0, modelHdr]];
  const fReq = JSON.stringify([null, JSON.stringify(inner)]);
  const body = new URLSearchParams({ "f.req": fReq, at: snlm }).toString();
  const url = new URL(`${BASE}/_/BardChatUi/data/assistant.lamda.BardFrontendService/StreamGenerate`);
  url.searchParams.set("bl", "boq_assistant-bard-web-server");
  url.searchParams.set("_reqid", reqId);
  url.searchParams.set("rt", "c");

  const resp = await fetch(url, {
    method: "POST",
    headers: headers(session, "application/x-www-form-urlencoded;charset=UTF-8"),
    body,
  });
  if (!resp.ok || !resp.body) {
    const text = await resp.text();
    throw new Error(`gemini completion ${resp.status}: ${text.slice(0, 400)}`);
  }

  let last = "";
  const reader = resp.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let nl: number;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line || line.startsWith(")]}'") || /^\d+$/.test(line)) continue;
      let outer: unknown;
      try {
        outer = JSON.parse(line);
      } catch {
        continue;
      }
      if (!Array.isArray(outer)) continue;
      for (const row of outer) {
        if (!Array.isArray(row) || row.length < 3) continue;
        const innerPayload = row[2];
        if (typeof innerPayload !== "string") continue;
        let data: unknown;
        try {
          data = JSON.parse(innerPayload);
        } catch {
          continue;
        }
        if (!Array.isArray(data) || data.length <= 4) continue;
        const cands = data[4];
        if (!Array.isArray(cands) || !cands.length) continue;
        const first = cands[0];
        if (!Array.isArray(first) || first.length < 2 || !first[1]) continue;
        const arr = first[1];
        if (!Array.isArray(arr) || !arr.length || typeof arr[0] !== "string") continue;
        const text = arr[0] as string;
        if (text.startsWith(last)) {
          const delta = text.slice(last.length);
          last = text;
          if (delta) yield { delta };
        } else {
          last = text;
          yield { delta: text };
        }
      }
    }
  }
}
