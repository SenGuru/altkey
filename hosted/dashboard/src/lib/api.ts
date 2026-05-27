// Dashboard API client. All paths hit the Workers backend.

export const API_BASE =
  (typeof import.meta !== "undefined" && (import.meta as any).env?.PUBLIC_API_BASE) ||
  "https://api.altkey.app";

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`${API_BASE}${path}`, {
    credentials: "include",
    ...init,
    headers: { "content-type": "application/json", ...(init?.headers as Record<string, string>) },
  });
  if (!r.ok) {
    const text = await r.text();
    throw new Error(`${r.status}: ${text}`);
  }
  return (await r.json()) as T;
}

export async function unlock(userId: string, verifierB64: string): Promise<{ ok: boolean; plan: string }> {
  return await req("/api/unlock", {
    method: "POST",
    body: JSON.stringify({ user_id: userId, verifier_b64: verifierB64 }),
  });
}

export async function accountStatus(): Promise<{
  user_id: string;
  plan: string;
  sessions: Array<{ provider: string; stale: number; updated_at: number }>;
}> {
  return await req("/api/account/status");
}

export async function listKeys(): Promise<{
  keys: Array<{ key_prefix: string; label: string | null; created_at: number }>;
}> {
  return await req("/api/keys");
}

export async function mintKey(label?: string): Promise<{ key: string }> {
  return await req("/api/keys", { method: "POST", body: JSON.stringify({ label: label ?? "" }) });
}

export async function revokeKey(key: string): Promise<{ ok: boolean }> {
  return await req("/api/keys/revoke", { method: "POST", body: JSON.stringify({ key }) });
}

export async function logout(): Promise<{ ok: boolean }> {
  return await req("/api/logout", { method: "POST" });
}
