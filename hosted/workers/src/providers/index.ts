import * as claude from "./claude";
import * as chatgpt from "./chatgpt";
import * as gemini from "./gemini";

export interface SessionBlob {
  cookies: Array<{ name: string; value: string; domain: string }>;
  user_agent: string;
}

export interface ProviderModule {
  NAME: string;
  PROVIDER_ID: string;
  MODELS: string[];
  stream: (req: ChatRequest, session: SessionBlob) => AsyncIterable<{ delta: string }>;
}

export interface ChatRequest {
  model: string;
  messages: Array<{
    role: string;
    content: string | Array<{ type?: string; text?: string }>;
  }>;
  stream?: boolean;
}

const PREFIXES: Array<[string, ProviderModule]> = [
  ["claude", claude as ProviderModule],
  ["gpt", chatgpt as ProviderModule],
  ["o1", chatgpt as ProviderModule],
  ["o3", chatgpt as ProviderModule],
  ["o4", chatgpt as ProviderModule],
  ["chatgpt", chatgpt as ProviderModule],
  ["gemini", gemini as ProviderModule],
];

export function forModel(model: string): ProviderModule | null {
  const m = model.toLowerCase();
  for (const [prefix, mod] of PREFIXES) {
    if (m.startsWith(prefix)) return mod;
  }
  return null;
}

export function listModels(): Array<{ id: string; object: "model"; owned_by: string }> {
  const out: Array<{ id: string; object: "model"; owned_by: string }> = [];
  for (const mod of [claude, chatgpt, gemini] as ProviderModule[]) {
    for (const id of mod.MODELS) out.push({ id, object: "model", owned_by: mod.NAME });
  }
  return out;
}

export function cookieHeader(session: SessionBlob, domains: string[]): string {
  const parts: string[] = [];
  for (const c of session.cookies) {
    const dom = (c.domain || "").replace(/^\./, "");
    if (domains.some((d) => dom.endsWith(d))) parts.push(`${c.name}=${c.value}`);
  }
  return parts.join("; ");
}

export function flattenContent(content: string | Array<{ type?: string; text?: string }>): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter((p) => p && (p.type === "text" || p.text != null))
      .map((p) => p.text ?? "")
      .join("");
  }
  return String(content);
}
