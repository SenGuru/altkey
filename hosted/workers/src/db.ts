// Typed D1 access. Every callsite goes through these helpers.

export interface UserRow {
  id: string;
  argon_salt: ArrayBuffer;
  argon_verifier: ArrayBuffer;
  hmac_pub: ArrayBuffer;
  wrapped_k_session: ArrayBuffer | null;
  proxy_k_session: ArrayBuffer | null;
  plan: string;
  created_at: number;
  killed_at: number | null;
}

export interface SessionRow {
  user_id: string;
  provider: string;
  ciphertext: ArrayBuffer;
  nonce: ArrayBuffer;
  stale: number;
  updated_at: number;
}

export interface ApiKeyRow {
  key_hash: ArrayBuffer;
  user_id: string;
  label: string | null;
  created_at: number;
  revoked_at: number | null;
}

export async function getUser(db: D1Database, userId: string): Promise<UserRow | null> {
  return await db
    .prepare("SELECT * FROM users WHERE id = ?")
    .bind(userId)
    .first<UserRow>();
}

export async function insertUser(
  db: D1Database,
  row: Omit<UserRow, "wrapped_k_session" | "proxy_k_session" | "plan" | "created_at" | "killed_at">,
): Promise<void> {
  await db
    .prepare(
      `INSERT INTO users (id, argon_salt, argon_verifier, hmac_pub, plan, created_at)
       VALUES (?, ?, ?, ?, 'free', ?)`,
    )
    .bind(row.id, row.argon_salt, row.argon_verifier, row.hmac_pub, Math.floor(Date.now() / 1000))
    .run();
}

export async function setUserSessionKeys(
  db: D1Database,
  userId: string,
  wrapped: Uint8Array,
  proxy: Uint8Array,
): Promise<void> {
  await db
    .prepare("UPDATE users SET wrapped_k_session = ?, proxy_k_session = ? WHERE id = ?")
    .bind(wrapped, proxy, userId)
    .run();
}

export async function setUserPlan(db: D1Database, userId: string, plan: string): Promise<void> {
  await db.prepare("UPDATE users SET plan = ? WHERE id = ?").bind(plan, userId).run();
}

export async function killUser(db: D1Database, userId: string, ts: number): Promise<void> {
  await db.prepare("UPDATE users SET killed_at = ? WHERE id = ?").bind(ts, userId).run();
}

export async function upsertSession(
  db: D1Database,
  row: { user_id: string; provider: string; ciphertext: Uint8Array; nonce: Uint8Array; updated_at: number },
): Promise<void> {
  await db
    .prepare(
      `INSERT INTO sessions (user_id, provider, ciphertext, nonce, stale, updated_at)
       VALUES (?, ?, ?, ?, 0, ?)
       ON CONFLICT(user_id, provider) DO UPDATE SET
         ciphertext = excluded.ciphertext,
         nonce = excluded.nonce,
         stale = 0,
         updated_at = excluded.updated_at
       WHERE excluded.updated_at > sessions.updated_at`,
    )
    .bind(row.user_id, row.provider, row.ciphertext, row.nonce, row.updated_at)
    .run();
}

export async function getSession(
  db: D1Database,
  userId: string,
  provider: string,
): Promise<SessionRow | null> {
  return await db
    .prepare("SELECT * FROM sessions WHERE user_id = ? AND provider = ?")
    .bind(userId, provider)
    .first<SessionRow>();
}

export async function markSessionStale(db: D1Database, userId: string, provider: string): Promise<void> {
  await db
    .prepare("UPDATE sessions SET stale = 1 WHERE user_id = ? AND provider = ?")
    .bind(userId, provider)
    .run();
}

export async function listSessions(db: D1Database, userId: string): Promise<Pick<SessionRow, "provider" | "stale" | "updated_at">[]> {
  const r = await db
    .prepare("SELECT provider, stale, updated_at FROM sessions WHERE user_id = ?")
    .bind(userId)
    .all<Pick<SessionRow, "provider" | "stale" | "updated_at">>();
  return r.results ?? [];
}

export async function insertApiKey(
  db: D1Database,
  keyHash: Uint8Array,
  userId: string,
  label: string,
): Promise<void> {
  await db
    .prepare("INSERT INTO api_keys (key_hash, user_id, label, created_at) VALUES (?, ?, ?, ?)")
    .bind(keyHash, userId, label || null, Math.floor(Date.now() / 1000))
    .run();
}

export async function findApiKeyOwner(
  db: D1Database,
  keyHash: Uint8Array,
): Promise<{ user_id: string } | null> {
  return await db
    .prepare("SELECT user_id FROM api_keys WHERE key_hash = ? AND revoked_at IS NULL")
    .bind(keyHash)
    .first<{ user_id: string }>();
}

export async function revokeApiKey(db: D1Database, keyHash: Uint8Array): Promise<void> {
  await db
    .prepare("UPDATE api_keys SET revoked_at = ? WHERE key_hash = ?")
    .bind(Math.floor(Date.now() / 1000), keyHash)
    .run();
}

export async function listApiKeysForUser(
  db: D1Database,
  userId: string,
): Promise<{ key_hash: ArrayBuffer; label: string | null; created_at: number }[]> {
  const r = await db
    .prepare(
      `SELECT key_hash, label, created_at FROM api_keys
       WHERE user_id = ? AND revoked_at IS NULL
       ORDER BY created_at DESC`,
    )
    .bind(userId)
    .all<{ key_hash: ArrayBuffer; label: string | null; created_at: number }>();
  return r.results ?? [];
}

export async function recordUsage(
  db: D1Database,
  userId: string,
  provider: string,
  model: string,
  bytes: number,
): Promise<void> {
  const day = todayYyyymmdd();
  await db
    .prepare(
      `INSERT INTO usage (user_id, day, provider, model, request_count, byte_count)
       VALUES (?, ?, ?, ?, 1, ?)
       ON CONFLICT(user_id, day, provider, model) DO UPDATE SET
         request_count = usage.request_count + 1,
         byte_count = usage.byte_count + excluded.byte_count`,
    )
    .bind(userId, day, provider, model, bytes)
    .run();
}

export async function usageToday(
  db: D1Database,
  userId: string,
  provider?: string,
): Promise<number> {
  const day = todayYyyymmdd();
  if (provider) {
    const r = await db
      .prepare(
        "SELECT SUM(request_count) AS c FROM usage WHERE user_id = ? AND day = ? AND provider = ?",
      )
      .bind(userId, day, provider)
      .first<{ c: number | null }>();
    return r?.c ?? 0;
  }
  const r = await db
    .prepare("SELECT SUM(request_count) AS c FROM usage WHERE user_id = ? AND day = ?")
    .bind(userId, day)
    .first<{ c: number | null }>();
  return r?.c ?? 0;
}

export async function insertPayment(
  db: D1Database,
  row: {
    id: string;
    user_id: string;
    processor: string;
    external_id: string;
    status: string;
    amount_cents: number | null;
  },
): Promise<{ inserted: boolean }> {
  try {
    await db
      .prepare(
        `INSERT INTO payments (id, user_id, processor, external_id, status, amount_cents, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)`,
      )
      .bind(row.id, row.user_id, row.processor, row.external_id, row.status, row.amount_cents, Math.floor(Date.now() / 1000))
      .run();
    return { inserted: true };
  } catch (e) {
    // UNIQUE (processor, external_id) collision = idempotent replay; treat as success.
    return { inserted: false };
  }
}

function todayYyyymmdd(): number {
  const d = new Date();
  return d.getUTCFullYear() * 10000 + (d.getUTCMonth() + 1) * 100 + d.getUTCDate();
}
