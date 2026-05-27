import importlib

import pytest


@pytest.fixture
def store(monkeypatch, tmp_path):
    monkeypatch.setenv("ALTKEY_HOME", str(tmp_path))
    from app import store as store_module
    importlib.reload(store_module)
    store_module.init()
    return store_module


def test_session_roundtrip(store):
    payload = {"cookies": [{"name": "sessionKey", "value": "abc"}], "user_agent": "UA"}
    store.save_session("claude", payload)
    got = store.load_session("claude")
    assert got == payload


def test_session_missing(store):
    assert store.load_session("claude") is None


def test_session_overwrite(store):
    store.save_session("claude", {"cookies": [], "user_agent": "v1"})
    store.save_session("claude", {"cookies": [], "user_agent": "v2"})
    assert store.load_session("claude")["user_agent"] == "v2"


def test_delete_session(store):
    store.save_session("claude", {"cookies": [], "user_agent": "x"})
    store.delete_session("claude")
    assert store.load_session("claude") is None


def test_list_sessions(store):
    store.save_session("claude", {"cookies": [], "user_agent": "x"})
    store.save_session("chatgpt", {"cookies": [], "user_agent": "y"})
    rows = store.list_sessions()
    providers = {r["provider"] for r in rows}
    assert providers == {"claude", "chatgpt"}


def test_api_key_lifecycle(store):
    key = store.mint_key("test-label")
    assert key.startswith("sk-alt-")
    assert store.key_exists(key)
    store.revoke_key(key)
    assert not store.key_exists(key)


def test_list_keys_contains_minted(store):
    k1 = store.mint_key("one")
    k2 = store.mint_key("two")
    keys = store.list_keys()
    assert {k["key"] for k in keys} == {k1, k2}
    assert {k["label"] for k in keys} == {"one", "two"}
