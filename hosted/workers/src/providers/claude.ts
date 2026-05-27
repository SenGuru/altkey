import { cookieHeader, flattenContent, type ChatRequest, type SessionBlob } from "./index";

export const NAME = "claude";
export const PROVIDER_ID = "claude";
export const MODELS = [
  "claude-opus-4-5",
  "claude-sonnet-4-5",
  "claude-haiku-4-5",
  "claude-3-5-sonnet-20241022",
  "claude-3-5-haiku-20241022",
];

const BASE = "https://claude.ai";
const DOMAINS = ["claude.ai"];

function headers(session: SessionBlob): HeadersInit {
  return {
    "User-Agent": session.user_agent || "Mozilla/5.0",
    Accept: "text/event-stream, application/json",
    "Accept-Language": "en-US,en;q=0.9",
    "Content-Type": "application/json",
    Origin: BASE,
    Referer: `${BASE}/chats`,
    Cookie: cookieHeader(session, DOMAINS),
  };
}

async function organizationId(session: SessionBlob): Promise<string> {
  const r = await fetch(`${BASE}/api/organizations`, { headers: headers(session) });
  if (!r.ok) throw new Error(`claude /organizations ${r.status}`);
  const orgs = (await r.json()) as Array<{ uuid: string; capabilities?: string[] }>;
  if (!orgs.length) throw new Error("no claude organizations");
  const withChat = orgs.find((o) => (o.capabilities ?? []).includes("chat"));
  return (withChat ?? orgs[0]!).uuid;
}

function buildPrompt(messages: ChatRequest["messages"]): { system: string; prompt: string } {
  const sysParts: string[] = [];
  const convo: string[] = [];
  for (const m of messages) {
    if (m.role === "system") {
      sysParts.push(flattenContent(m.content));
      continue;
    }
    const tag = m.role === "user" ? "Human" : m.role === "assistant" ? "Assistant" : m.role;
    convo.push(`${tag}: ${flattenContent(m.content)}`);
  }
  let prompt = convo.join("\n\n");
  if (!prompt.endsWith("Assistant:")) prompt += "\n\nAssistant:";
  return { system: sysParts.join("\n\n"), prompt };
}

export async function* stream(req: ChatRequest, session: SessionBlob): AsyncIterable<{ delta: string }> {
  const model = req.model;
  const { system, prompt } = buildPrompt(req.messages);
  const org = await organizationId(session);

  const conv = crypto.randomUUID();
  const createRes = await fetch(`${BASE}/api/organizations/${org}/chat_conversations`, {
    method: "POST",
    headers: headers(session),
    body: JSON.stringify({ uuid: conv, name: "" }),
  });
  if (!createRes.ok) throw new Error(`claude conversation create ${createRes.status}`);

  const payload: Record<string, unknown> = {
    prompt,
    parent_message_uuid: "00000000-0000-4000-8000-000000000000",
    timezone: "America/Los_Angeles",
    attachments: [],
    files: [],
    sync_sources: [],
    rendering_mode: "messages",
    model,
  };
  if (system) payload.personalized_styles = [{ key: "custom", instructions: system }];

  try {
    const resp = await fetch(`${BASE}/api/organizations/${org}/chat_conversations/${conv}/completion`, {
      method: "POST",
      headers: headers(session),
      body: JSON.stringify(payload),
    });
    if (!resp.ok || !resp.body) {
      const body = await resp.text();
      throw new Error(`claude completion ${resp.status}: ${body.slice(0, 400)}`);
    }
    for await (const evt of sseEvents(resp.body)) {
      const t = evt.type;
      if (t === "completion" && typeof evt.completion === "string" && evt.completion) {
        yield { delta: evt.completion };
      } else if (t === "content_block_delta") {
        const d = (evt as { delta?: { type?: string; text?: string } }).delta;
        if (d && d.type === "text_delta" && d.text) yield { delta: d.text };
      } else if (t === "message_stop") {
        break;
      }
    }
  } finally {
    fetch(`${BASE}/api/organizations/${org}/chat_conversations/${conv}`, {
      method: "DELETE",
      headers: headers(session),
    }).catch(() => {});
  }
}

async function* sseEvents(body: ReadableStream<Uint8Array>): AsyncIterable<Record<string, unknown>> {
  const reader = body.getReader();
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
      try {
        yield JSON.parse(data);
      } catch {
        // ignore
      }
    }
  }
}
