from . import claude, chatgpt, gemini

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
    out = []
    for mod in (claude, chatgpt, gemini):
        for mid in mod.MODELS:
            out.append({"id": mid, "object": "model", "owned_by": mod.NAME})
    return out
