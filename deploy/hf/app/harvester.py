import asyncio
from typing import Callable

from . import store

_LOGIN_URLS = {
    "claude": "https://claude.ai/login",
    "chatgpt": "https://chatgpt.com/",
    "gemini": "https://gemini.google.com/app",
}

_DONE_URL_HINTS = {
    "claude": ("claude.ai/chat", "claude.ai/new", "claude.ai/projects", "claude.ai/recents"),
    "chatgpt": ("chatgpt.com/c", "chatgpt.com/?", "chatgpt.com/g"),
    "gemini": ("gemini.google.com/app",),
}

_COOKIE_KEYS = {
    "claude": ("sessionKey",),
    "chatgpt": ("__Secure-next-auth.session-token",),
    "gemini": ("__Secure-1PSID", "__Secure-1PSIDTS"),
}


async def harvest(provider: str, on_status: Callable[[str], None] | None = None) -> dict:
    if provider not in _LOGIN_URLS:
        raise ValueError(f"unknown provider: {provider}")

    def say(msg: str) -> None:
        if on_status:
            on_status(msg)

    # Imported lazily so the server runs in environments without Playwright
    # installed (e.g. the hosted relay only imports cookies, never harvests).
    try:
        from playwright.async_api import async_playwright
    except ImportError:
        raise RuntimeError("Playwright not installed — browser login unavailable on this host")

    profile_dir = store.DATA_DIR / "profiles" / provider
    profile_dir.mkdir(parents=True, exist_ok=True)

    # Anti-detection: use the real installed Chrome (far less likely to trip
    # Cloudflare Turnstile than Playwright's bundled Chromium), drop the
    # automation flags, and strip navigator.webdriver before any page script
    # runs. A persistent profile also accumulates trust over repeat logins.
    launch_kwargs = dict(
        user_data_dir=str(profile_dir),
        headless=False,
        no_viewport=True,
        ignore_default_args=["--enable-automation"],
        args=[
            "--disable-blink-features=AutomationControlled",
            "--start-maximized",
            "--no-default-browser-check",
            "--disable-features=IsolateOrigins,site-per-process",
        ],
    )

    async with async_playwright() as pw:
        try:
            ctx = await pw.chromium.launch_persistent_context(channel="chrome", **launch_kwargs)
            say("using installed Chrome")
        except Exception:
            ctx = await pw.chromium.launch_persistent_context(**launch_kwargs)
            say("installed Chrome not found — using bundled Chromium (captcha more likely)")

        await ctx.add_init_script(
            """
            Object.defineProperty(navigator, 'webdriver', {get: () => undefined});
            Object.defineProperty(navigator, 'languages', {get: () => ['en-US', 'en']});
            Object.defineProperty(navigator, 'plugins', {get: () => [1, 2, 3, 4, 5]});
            window.chrome = window.chrome || { runtime: {} };
            """
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
