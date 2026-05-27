// SSE writer for /v1/chat/completions streaming.

export function sseStream(
  source: AsyncIterable<Record<string, unknown> | "DONE">,
): ReadableStream<Uint8Array> {
  const enc = new TextEncoder();
  return new ReadableStream({
    async start(controller) {
      try {
        for await (const evt of source) {
          if (evt === "DONE") {
            controller.enqueue(enc.encode("data: [DONE]\n\n"));
          } else {
            controller.enqueue(enc.encode(`data: ${JSON.stringify(evt)}\n\n`));
          }
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        controller.enqueue(
          enc.encode(`data: ${JSON.stringify({ error: { message: msg, type: "upstream_error" } })}\n\n`),
        );
        controller.enqueue(enc.encode("data: [DONE]\n\n"));
      } finally {
        controller.close();
      }
    },
  });
}

export const SSE_HEADERS = {
  "content-type": "text/event-stream",
  "cache-control": "no-cache, no-transform",
  "x-accel-buffering": "no",
};
