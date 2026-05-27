// OpenAI chat-completion chunk shape.

export function openaiChunk(model: string, delta: string, finish?: "stop" | null): Record<string, unknown> {
  return {
    id: `chatcmpl-${crypto.randomUUID().replace(/-/g, "").slice(0, 24)}`,
    object: "chat.completion.chunk",
    created: Math.floor(Date.now() / 1000),
    model,
    choices: [
      {
        index: 0,
        delta: delta ? { content: delta } : {},
        finish_reason: finish ?? null,
      },
    ],
  };
}

export function openaiCompletion(model: string, content: string): Record<string, unknown> {
  return {
    id: `chatcmpl-${crypto.randomUUID().replace(/-/g, "").slice(0, 24)}`,
    object: "chat.completion",
    created: Math.floor(Date.now() / 1000),
    model,
    choices: [
      {
        index: 0,
        message: { role: "assistant", content },
        finish_reason: "stop",
      },
    ],
    usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  };
}
