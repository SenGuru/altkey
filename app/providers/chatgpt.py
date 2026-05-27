from typing import AsyncIterator

NAME = "chatgpt"
MODELS = ["gpt-4o", "gpt-4o-mini", "gpt-4-1", "o1", "o3", "o4-mini"]


async def stream(req: dict) -> AsyncIterator[dict]:
    # TODO: implement chat.openai.com/backend-api/conversation flow.
    # Requires: session cookie, accessToken from /api/auth/session,
    # Arkose token + WSS for non-free models. Stream returns SSE with
    # 'message' events containing message.content.parts deltas.
    raise NotImplementedError("chatgpt provider stub — not implemented yet")
    yield  # type: ignore[unreachable]
