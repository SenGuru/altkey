from . import claude, chatgpt, gemini, claude_oauth
from .. import store


def _claude_module():
    """Prefer the OAuth (real-API) Claude path when a CLI token is connected;
    fall back to the chat-backend relay otherwise."""
    if store.load_session("claude_oauth"):
        return claude_oauth
    return claude


_BY_PREFIX = [
    ("gpt", chatgpt),
    ("o1", chatgpt),
    ("o3", chatgpt),
    ("o4", chatgpt),
    ("chatgpt", chatgpt),
    ("gemini", gemini),
]


def for_model(model: str):
    m = model.lower()
    if m.startswith("claude"):
        return _claude_module()
    for prefix, mod in _BY_PREFIX:
        if m.startswith(prefix):
            return mod
    return None


def list_models() -> list[dict]:
    """Prefer each provider's live-detected models; fall back to static."""
    out = []
    seen = set()
    claude_mod = _claude_module()
    cache_name = "claude_oauth" if claude_mod is claude_oauth else "claude"
    plan = [(claude_mod, cache_name), (chatgpt, "chatgpt"), (gemini, "gemini")]
    for mod, name in plan:
        cached = store.load_detected_models(name)
        detected = cached["models"] if cached else []
        for mid in [*detected, *mod.MODELS]:
            if mid and ("claude", mid) not in seen and (mod.NAME, mid) not in seen:
                seen.add((mod.NAME, mid))
                out.append({"id": mid, "object": "model", "owned_by": mod.NAME})
    return out


async def detect_all() -> dict:
    results = {}
    plan = [("claude", _claude_module()), ("chatgpt", chatgpt), ("gemini", gemini)]
    for label, mod in plan:
        sess_name = "claude_oauth" if mod is claude_oauth else label
        if not store.load_session(sess_name):
            continue
        try:
            results[label] = await mod.detect(store.load_session(sess_name))
        except Exception as e:
            results[label] = {"error": str(e)[:200]}
    return results


async def detect_one(provider: str) -> list[str]:
    mapping = {"claude": claude, "claude_oauth": claude_oauth, "chatgpt": chatgpt, "gemini": gemini}
    mod = mapping.get(provider)
    if not mod:
        return []
    sess = store.load_session(provider)
    if not sess:
        return []
    return await mod.detect(sess)
