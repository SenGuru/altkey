import asyncio
from typing import Callable

from playwright.async_api import async_playwright

from . import store

_LOGIN_URLS = {
    "claude": "https://claude.ai/login",
    "chatgpt": "https://chat.openai.com/",
    "gemini": "https://gemini.google.com/",
}

_DONE_URL_HINTS = {
    "claude": ("claude.ai/chat", "claude.ai/new", "claude.ai/projects", "claude.ai/recents"),
    "chatgpt": ("chat.openai.com/c", "chat.openai.com/?", "chatgpt.com/c", "chatgpt.com/?"),
    "gemini": ("gemini.google.com/app",),
}

_COOKIE_KEYS = {
    "claude": ("sessionKey",),
    "chatgpt": ("__Secure-next-auth.session-token",),
    "gemini": ("__Secure-1PSID", "__Secure-1PSIDTS", "__Secure-1PSIDCC"),
}


async def harvest(provider: str, on_status: Callable[[str], None] | None = None) -> dict:
    if provider not in _LOGIN_URLS:
        raise ValueError(f"unknown provider: {provider}")

    def say(msg: str) -> None:
        if on_status:
            on_status(msg)

    profile_dir = store.DATA_DIR / "profiles" / provider
    profile_dir.mkdir(parents=True, exist_ok=True)

    async with async_playwright() as pw:
        ctx = await pw.chromium.launch_persistent_context(
            user_data_dir=str(profile_dir),
            headless=False,
            viewport={"width": 1100, "height": 800},
        )
        page = ctx.pages[0] if ctx.pages else await ctx.new_page()
        say(f"opening {provider} login")
        await page.goto(_LOGIN_URLS[provider])

        deadline = asyncio.get_event_loop().time() + 600
        cookies: list[dict] = []
        ua = ""
        hints = _DONE_URL_HINTS[provider]
        needed = set(_COOKIE_KEYS[provider])

        while asyncio.get_event_loop().time() < deadline:
            await asyncio.sleep(1.5)
            url = page.url
            cookies = await ctx.cookies()
            have = {c["name"] for c in cookies}
            if any(h in url for h in hints) and needed.issubset(have):
                ua = await page.evaluate("navigator.userAgent")
                say("captured session")
                break
        else:
            await ctx.close()
            raise TimeoutError("login window timed out; nothing saved")

        data = {
            "cookies": [
                {k: c.get(k) for k in ("name", "value", "domain", "path", "expires", "httpOnly", "secure", "sameSite")}
                for c in cookies
            ],
            "user_agent": ua,
        }
        store.save_session(provider, data)
        await ctx.close()
        return {"provider": provider, "cookie_count": len(cookies)}


def cookie_header(session: dict, domains: tuple[str, ...]) -> str:
    parts = []
    for c in session.get("cookies", []):
        dom = (c.get("domain") or "").lstrip(".")
        if any(dom.endswith(d) for d in domains):
            parts.append(f"{c['name']}={c['value']}")
    return "; ".join(parts)
