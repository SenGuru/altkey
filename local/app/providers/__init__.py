from . import chatgpt, claude_oauth
# PARKED: Gemini provider is intentionally disconnected from active routing.
# See local/spikes/gemini_findings.md — chat+vision via OAuth works, but Google
# walled off image gen from the OAuth path (Pro sub only covers gemini.google.com
# web app; API/CLI/Vertex all bill per image). Without parity, parked.
# Receipt code preserved at local/app/providers/gemini.py.
# from . import gemini  # noqa: parked
from .. import store


_BY_PREFIX = [
    ("gpt", chatgpt),
    ("o1", chatgpt),
    ("o3", chatgpt),
    ("o4", chatgpt),
    ("chatgpt", chatgpt),
    # ("gemini", gemini),  # parked
]


def for_model(model: str):
    m = model.lower()
    if m.startswith("claude"):
        return claude_oauth
    for prefix, mod in _BY_PREFIX:
        if m.startswith(prefix):
            return mod
    return None


def list_models() -> list[dict]:
    """Prefer each provider's live-detected models; fall back to static."""
    out = []
    seen = set()
    plan = [(claude_oauth, "claude_oauth"), (chatgpt, "chatgpt")]
    # parked: (gemini, "gemini")
    for mod, name in plan:
        cached = store.load_detected_models(name)
        detected = cached["models"] if cached else []
        for mid in [*detected, *mod.MODELS]:
            if mid and (mod.NAME, mid) not in seen:
                seen.add((mod.NAME, mid))
                out.append({"id": mid, "object": "model", "owned_by": mod.NAME})
    return out


async def detect_all() -> dict:
    results = {}
    plan = [("claude", claude_oauth, "claude_oauth"), ("chatgpt", chatgpt, "chatgpt")]
    # parked: ("gemini", gemini, "gemini")
    for label, mod, sess_name in plan:
        if not store.load_session(sess_name):
            continue
        try:
            results[label] = await mod.detect(store.load_session(sess_name))
        except Exception as e:
            results[label] = {"error": str(e)[:200]}
    return results


async def detect_one(provider: str) -> list[str]:
    mapping = {"claude_oauth": claude_oauth, "chatgpt": chatgpt}
    # parked: "gemini": gemini
    mod = mapping.get(provider)
    if not mod:
        return []
    sess = store.load_session(provider)
    if not sess:
        return []
    return await mod.detect(sess)
