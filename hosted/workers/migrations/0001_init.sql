-- altkey D1 schema. See docs/superpowers/specs/2026-05-27-altkey-design.md §4.

CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY,
  argon_salt BLOB NOT NULL,
  argon_verifier BLOB NOT NULL,
  hmac_pub BLOB NOT NULL,
  wrapped_k_session BLOB,
  proxy_k_session BLOB,
  plan TEXT NOT NULL DEFAULT 'free',
  created_at INTEGER NOT NULL,
  killed_at INTEGER
);

CREATE TABLE IF NOT EXISTS sessions (
  user_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  ciphertext BLOB NOT NULL,
  nonce BLOB NOT NULL,
  stale INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, provider)
);

CREATE TABLE IF NOT EXISTS api_keys (
  key_hash BLOB PRIMARY KEY,
  user_id TEXT NOT NULL,
  label TEXT,
  created_at INTEGER NOT NULL,
  revoked_at INTEGER
);

CREATE TABLE IF NOT EXISTS usage (
  user_id TEXT NOT NULL,
  day INTEGER NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  request_count INTEGER NOT NULL,
  byte_count INTEGER NOT NULL,
  PRIMARY KEY (user_id, day, provider, model)
);

CREATE TABLE IF NOT EXISTS payments (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  processor TEXT NOT NULL,
  external_id TEXT NOT NULL,
  status TEXT NOT NULL,
  amount_cents INTEGER,
  created_at INTEGER NOT NULL,
  UNIQUE (processor, external_id)
);

CREATE TABLE IF NOT EXISTS sessions_log (
  user_id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  event TEXT NOT NULL,
  ip_hash BLOB
);

CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at);
CREATE INDEX IF NOT EXISTS idx_usage_user_day ON usage(user_id, day);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_payments_user ON payments(user_id, created_at);
