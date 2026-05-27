from typing import AsyncIterator

NAME = "gemini"
MODELS = ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash"]


async def stream(req: dict) -> AsyncIterator[dict]:
    # TODO: implement gemini.google.com BardChatUi flow.
    # Endpoint: /_/BardChatUi/data/assistant.lamda.BardFrontendService/StreamGenerate
    # Requires: __Secure-1PSID/1PSIDTS/1PSIDCC cookies + SNlM0e token scraped
    # from page HTML on first call. Response is chunked JSON arrays.
    raise NotImplementedError("gemini provider stub — not implemented yet")
    yield  # type: ignore[unreachable]
