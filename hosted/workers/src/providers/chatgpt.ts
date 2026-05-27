import { cookieHeader, flattenContent, type ChatRequest, type SessionBlob } from "./index";

export const NAME = "chatgpt";
export const PROVIDER_ID = "chatgpt";
export const MODELS = [
  "gpt-4o",
  "gpt-4o-mini",
  "gpt-4-1",
  "gpt-4-1-mini",
  "o1",
  "o3",
  "o4-mini",
  "chatgpt-4o-latest",
];

const BASE = "https://chatgpt.com";
const DOMAINS = ["chatgpt.com", "chat.openai.com", "openai.com"];

function headers(session: SessionBlob, accessToken?: string, oaiDevice?: string): HeadersInit {
  const h: Record<string, string> = {
    "User-Agent": session.user_agent || "Mozilla/5.0",
    Accept: "text/event-stream",
    "Accept-Language": "en-US,en;q=0.9",
    Origin: BASE,
    Referer: `${BASE}/`,
    Cookie: cookieHeader(session, DOMAINS),
  };
  if (accessToken) {
    h.Authorization = `Bearer ${accessToken}`;
    h["Content-Type"] = "application/json";
  }
  if (oaiDevice) h["OAI-Device-Id"] = oaiDevice;
  return h;
}

async function accessToken(session: SessionBlob): Promise<string> {
  const r = await fetch(`${BASE}/api/auth/session`, { headers: headers(session) });
  if (!r.ok) throw new Error(`chatgpt auth/session ${r.status} — re-connect in dashboard`);
  const data = (await r.json()) as { accessToken?: string };
  if (!data.accessToken) throw new Error("chatgpt session expired — re-connect");
  return data.accessToken;
}

function deviceId(session: SessionBlob): string {
  const c = session.cookies.find((c) => c.name === "oai-did");
  return c?.value || crypto.randomUUID();
}

function toParts(messages: ChatRequest["messages"]): unknown[] {
  return messages.map((m) => ({
    id: crypto.randomUUID(),
    author: {
      role: m.role === "system" ? "system" : m.role === "assistant" ? "assistant" : "user",
    },
    content: { content_type: "text", parts: [flattenContent(m.content)] },
    metadata: {},
  }));
}

export async function* stream(req: ChatRequest, session: SessionBlob): AsyncIterable<{ delta: string }> {
  const token = await accessToken(session);
  const device = deviceId(session);
  const payload = {
    action: "next",
    messages: toParts(req.messages),
    parent_message_id: crypto.randomUUID(),
    model: req.model,
    timezone_offset_min: 420,
    history_and_training_disabled: false,
    conversation_mode: { kind: "primary_assistant" },
    force_paragen: false,
    force_rate_limit: false,
    suggestions: [],
  };

  const resp = await fetch(`${BASE}/backend-api/conversation`, {
    method: "POST",
    headers: headers(session, token, device),
    body: JSON.stringify(payload),
  });
  if (!resp.ok || !resp.body) {
    const body = await resp.text();
    throw new Error(`chatgpt completion ${resp.status}: ${body.slice(0, 400)}`);
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
      if (!line || !line.startsWith("data:")) continue;
      const data = line.slice(5).trim();
      if (!data || data === "[DONE]") continue;
      let evt: Record<string, unknown>;
      try {
        evt = JSON.parse(data);
      } catch {
        continue;
      }
      if (evt.type === "moderation") continue;
      const msg = (evt as { message?: { author?: { role?: string }; content?: { parts?: unknown[] } } }).message;
      if (!msg || msg.author?.role !== "assistant") continue;
      const parts = msg.content?.parts;
      if (!parts || !parts.length || typeof parts[0] !== "string") continue;
      const text = parts[0];
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
