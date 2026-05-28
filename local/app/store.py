import json
import os
import secrets
import sqlite3
import time
from pathlib import Path

from cryptography.fernet import Fernet

DATA_DIR = Path(os.environ.get("ALTKEY_HOME", Path.home() / ".altkey"))
DATA_DIR.mkdir(parents=True, exist_ok=True)
DB_PATH = DATA_DIR / "store.db"

_SERVICE = "altkey"
_KEYRING_USER = "fernet_master"


def _fernet() -> Fernet:
    # Priority: explicit env var (servers/headless) → OS keyring (desktop) →
    # a generated key persisted to disk (fallback when neither is available).
    env_key = os.environ.get("ALTKEY_FERNET_KEY")
    if env_key:
        return Fernet(env_key.encode())

    try:
        import keyring
        key = keyring.get_password(_SERVICE, _KEYRING_USER)
        if not key:
            key = Fernet.generate_key().decode()
            keyring.set_password(_SERVICE, _KEYRING_USER, key)
        return Fernet(key.encode())
    except Exception:
        pass

    # Last resort: a key file in DATA_DIR (used in containers without keyring).
    key_path = DATA_DIR / "fernet.key"
    if key_path.exists():
        return Fernet(key_path.read_bytes())
    key = Fernet.generate_key()
    key_path.write_bytes(key)
    return Fernet(key)


def _conn() -> sqlite3.Connection:
    c = sqlite3.connect(DB_PATH)
    c.execute("PRAGMA journal_mode=WAL")
    return c


def init() -> None:
    with _conn() as c:
        c.execute("""
            CREATE TABLE IF NOT EXISTS sessions (
                provider TEXT PRIMARY KEY,
                ciphertext BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            )
        """)
        c.execute("""
            CREATE TABLE IF NOT EXISTS api_keys (
                key TEXT PRIMARY KEY,
                label TEXT,
                created_at INTEGER NOT NULL
            )
        """)
        c.execute("""
            CREATE TABLE IF NOT EXISTS detected_models (
                provider TEXT PRIMARY KEY,
                models TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
        """)


def save_detected_models(provider: str, models: list[str]) -> None:
    with _conn() as c:
        c.execute(
            "INSERT OR REPLACE INTO detected_models (provider, models, updated_at) VALUES (?, ?, ?)",
            (provider, json.dumps(models), int(time.time())),
        )


def load_detected_models(provider: str) -> dict | None:
    try:
        with _conn() as c:
            row = c.execute(
                "SELECT models, updated_at FROM detected_models WHERE provider = ?", (provider,)
            ).fetchone()
    except sqlite3.OperationalError:
        return None  # table not created yet (init() not run)
    if not row:
        return None
    return {"models": json.loads(row[0]), "updated_at": row[1]}


def save_session(provider: str, data: dict) -> None:
    blob = _fernet().encrypt(json.dumps(data).encode())
    with _conn() as c:
        c.execute(
            "INSERT OR REPLACE INTO sessions (provider, ciphertext, updated_at) VALUES (?, ?, ?)",
            (provider, blob, int(time.time())),
        )


def load_session(provider: str) -> dict | None:
    with _conn() as c:
        row = c.execute(
            "SELECT ciphertext FROM sessions WHERE provider = ?", (provider,)
        ).fetchone()
    if not row:
        return None
    return json.loads(_fernet().decrypt(row[0]).decode())


def list_sessions() -> list[dict]:
    with _conn() as c:
        rows = c.execute("SELECT provider, updated_at FROM sessions").fetchall()
    return [{"provider": p, "updated_at": u} for p, u in rows]


def delete_session(provider: str) -> None:
    with _conn() as c:
        c.execute("DELETE FROM sessions WHERE provider = ?", (provider,))


def mint_key(label: str = "") -> str:
    key = "sk-alt-" + secrets.token_urlsafe(32)
    with _conn() as c:
        c.execute(
            "INSERT INTO api_keys (key, label, created_at) VALUES (?, ?, ?)",
            (key, label, int(time.time())),
        )
    return key


def key_exists(key: str) -> bool:
    with _conn() as c:
        return c.execute("SELECT 1 FROM api_keys WHERE key = ?", (key,)).fetchone() is not None


def list_keys() -> list[dict]:
    with _conn() as c:
        rows = c.execute("SELECT key, label, created_at FROM api_keys ORDER BY created_at DESC").fetchall()
    return [{"key": k, "label": l, "created_at": t} for k, l, t in rows]


def revoke_key(key: str) -> None:
    with _conn() as c:
        c.execute("DELETE FROM api_keys WHERE key = ?", (key,))
