//! Config = env + filesystem paths. Mirrors Python's behavior exactly.
use std::path::PathBuf;

pub fn admin_token() -> Option<String> {
    let t = std::env::var("ALTKEY_ADMIN_TOKEN").unwrap_or_default();
    if t.is_empty() { None } else { Some(t) }
}

pub fn home() -> PathBuf {
    dirs::home_dir().expect("no home dir")
}

pub fn altkey_dir() -> PathBuf {
    let d = home().join(".altkey");
    std::fs::create_dir_all(&d).ok();
    d
}

pub fn db_path() -> PathBuf {
    // Match Python reference exactly: ~/.altkey/store.db
    altkey_dir().join("store.db")
}

pub fn claude_creds_path() -> PathBuf {
    home().join(".claude").join(".credentials.json")
}

pub fn codex_creds_path() -> PathBuf {
    home().join(".codex").join("auth.json")
}
