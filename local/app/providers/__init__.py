from . import claude, chatgpt, gemini
from .. import store

_BY_PREFIX = [
    ("claude", claude),
    ("gpt", chatgpt),
    ("o1", chatgpt),
    ("o3", chatgpt),
    ("o4", chatgpt),
    ("chatgpt", chatgpt),
    ("gemini", gemini),
]


def for_model(model: str):
    m = model.lower()
    for prefix, mod in _BY_PREFIX:
        if m.startswith(prefix):
            return mod
    return None


def list_models() -> list[dict]:
    """Prefer each provider's live-detected models (cached on connect); fall
    back to the static catalog when nothing has been detected yet."""
    out = []
    seen = set()
    for mod in (claude, chatgpt, gemini):
        cached = store.load_detected_models(mod.NAME)
        detected = cached["models"] if cached else []
        # detected first (real availability), then static aliases for coverage
        for mid in [*detected, *mod.MODELS]:
            if mid and (mod.NAME, mid) not in seen:
                seen.add((mod.NAME, mid))
                out.append({"id": mid, "object": "model", "owned_by": mod.NAME})
    return out


async def detect_all() -> dict:
    """Run detection for every connected provider; returns {provider: models}."""
    results = {}
    for mod in (claude, chatgpt, gemini):
        sess = store.load_session(mod.NAME)
        if not sess:
            continue
        try:
            results[mod.NAME] = await mod.detect(sess)
        except Exception as e:
            results[mod.NAME] = {"error": str(e)[:200]}
    return results


async def detect_one(provider: str) -> list[str]:
    mod = {"claude": claude, "chatgpt": chatgpt, "gemini": gemini}.get(provider)
    if not mod:
        return []
    sess = store.load_session(provider)
    if not sess:
        return []
    return await mod.detect(sess)
