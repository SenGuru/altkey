//! SQLite store. Shares the Python reference's db file (~/.altkey/store.db) and
//! the api_keys + detected_models tables (plaintext, cross-compatible — keys
//! minted by Python validate here and vice versa).
//!
//! Sessions are NOT persisted here in the Rust engine — Phase 0 used Fernet
//! encryption tied to a Python keyring backend that's not worth porting. The
//! engine reads OAuth credentials directly from the CLI cred files
//! (~/.claude/.credentials.json + ~/.codex/auth.json) on every refresh cycle,
//! and caches access tokens in-memory. The user-visible connect-state is
//! "credentials present?" — same outcome, no encryption rebuild.
use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config;

static CONN: once_cell::sync::OnceCell<Arc<Mutex<Connection>>> = once_cell::sync::OnceCell::new();

fn conn() -> Arc<Mutex<Connection>> {
    CONN.get().expect("store::init() not called").clone()
}

pub fn init() -> Result<()> {
    let c = Connection::open(config::db_path())?;
    c.execute_batch("PRAGMA journal_mode=WAL;")?;
    c.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS api_keys (
            key TEXT PRIMARY KEY,
            label TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS detected_models (
            provider TEXT PRIMARY KEY,
            models TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        "#,
    )?;
    // Match Python's migration: add provider column if missing.
    let has_provider: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('api_keys') WHERE name='provider'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if has_provider == 0 {
        c.execute("ALTER TABLE api_keys ADD COLUMN provider TEXT", [])
            .ok();
    }
    CONN.set(Arc::new(Mutex::new(c)))
        .map_err(|_| anyhow::anyhow!("store already initialized"))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key: String,
    pub label: Option<String>,
    pub created_at: i64,
    pub provider: Option<String>,
}

pub fn mint_key(label: &str, provider: Option<&str>) -> Result<String> {
    let key = format!("sk-alt-{}", token_urlsafe(32));
    let now = unix_now();
    let c = conn();
    let c = c.lock();
    c.execute(
        "INSERT INTO api_keys (key, label, created_at, provider) VALUES (?, ?, ?, ?)",
        params![key, label, now, provider],
    )?;
    Ok(key)
}

pub fn key_exists(key: &str) -> bool {
    let c = conn();
    let c = c.lock();
    c.query_row("SELECT 1 FROM api_keys WHERE key = ?", params![key], |_| {
        Ok(())
    })
    .is_ok()
}

pub fn key_provider(key: &str) -> Option<String> {
    let c = conn();
    let c = c.lock();
    c.query_row(
        "SELECT provider FROM api_keys WHERE key = ?",
        params![key],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

pub fn list_keys() -> Vec<ApiKey> {
    let c = conn();
    let c = c.lock();
    let mut stmt = c
        .prepare("SELECT key, label, created_at, provider FROM api_keys ORDER BY created_at DESC")
        .expect("prepare");
    let rows = stmt
        .query_map([], |r| {
            Ok(ApiKey {
                key: r.get(0)?,
                label: r.get(1).ok(),
                created_at: r.get(2)?,
                provider: r.get(3).ok().flatten(),
            })
        })
        .expect("query_map");
    rows.filter_map(|r| r.ok()).collect()
}

pub fn revoke_key(key: &str) -> Result<()> {
    let c = conn();
    let c = c.lock();
    c.execute("DELETE FROM api_keys WHERE key = ?", params![key])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectedModels {
    pub models: Vec<String>,
    pub updated_at: i64,
}

pub fn save_detected_models(provider: &str, models: &[String]) -> Result<()> {
    let json_models = serde_json::to_string(models)?;
    let now = unix_now();
    let c = conn();
    let c = c.lock();
    c.execute(
        "INSERT OR REPLACE INTO detected_models (provider, models, updated_at) VALUES (?, ?, ?)",
        params![provider, json_models, now],
    )?;
    Ok(())
}

pub fn load_detected_models(provider: &str) -> Option<DetectedModels> {
    let c = conn();
    let c = c.lock();
    c.query_row(
        "SELECT models, updated_at FROM detected_models WHERE provider = ?",
        params![provider],
        |r| {
            let json: String = r.get(0)?;
            let updated_at: i64 = r.get(1)?;
            Ok((json, updated_at))
        },
    )
    .ok()
    .and_then(|(json, updated_at)| {
        serde_json::from_str::<Vec<String>>(&json)
            .ok()
            .map(|models| DetectedModels { models, updated_at })
    })
}

// ---- helpers ---------------------------------------------------------------

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn token_urlsafe(n_bytes: usize) -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut buf = vec![0u8; n_bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf)
}

pub fn now_unix() -> i64 {
    unix_now()
}

pub fn list_sessions() -> Vec<serde_json::Value> {
    // Phase 1: derive from cred-file presence — see crate::providers.
    crate::providers::live_sessions()
}

pub fn delete_session(_provider: &str) -> Result<()> {
    // Phase 1 no-op; on-disk cred files are owned by the user, not us.
    // (The Python engine wiped the encrypted DB row, which has no equivalent here.)
    Ok(())
}
