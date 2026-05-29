from app.providers import for_model, list_models
from app.providers import claude, chatgpt, gemini, claude_oauth


def test_for_model_routing_claude():
    # routes to a Claude provider — chat-relay OR oauth depending on what's connected
    assert for_model("claude-sonnet-4-5") in (claude, claude_oauth)
    assert for_model("claude-opus-4-5") in (claude, claude_oauth)
    assert for_model("claude-3-5-haiku-20241022") in (claude, claude_oauth)


def test_for_model_routing_chatgpt():
    assert for_model("gpt-4o") is chatgpt
    assert for_model("gpt-4-1-mini") is chatgpt
    assert for_model("o1") is chatgpt
    assert for_model("o3") is chatgpt
    assert for_model("o4-mini") is chatgpt
    assert for_model("chatgpt-4o-latest") is chatgpt


def test_for_model_routing_gemini():
    assert for_model("gemini-2.5-pro") is gemini
    assert for_model("gemini-1.5-pro") is gemini


def test_for_model_unknown():
    assert for_model("llama-3-70b") is None
    assert for_model("") is None


def test_list_models_contains_each_provider():
    ids = [m["id"] for m in list_models()]
    assert any(m.startswith("claude-") for m in ids)
    assert any(m.startswith("gpt-") for m in ids)
    assert any(m.startswith("gemini-") for m in ids)


def test_claude_flatten_basic():
    sys, prompt = claude._flatten([
        {"role": "system", "content": "be brief"},
        {"role": "user", "content": "hi"},
    ])
    assert sys == "be brief"
    assert prompt.endswith("Assistant:")
    assert "Human: hi" in prompt


def test_claude_flatten_assistant_turn():
    _, prompt = claude._flatten([
        {"role": "user", "content": "ping"},
        {"role": "assistant", "content": "pong"},
        {"role": "user", "content": "again"},
    ])
    assert "Human: ping" in prompt
    assert "Assistant: pong" in prompt
    assert prompt.endswith("Assistant:")


def test_claude_flatten_list_content():
    _, prompt = claude._flatten([
        {"role": "user", "content": [
            {"type": "text", "text": "part1 "},
            {"type": "text", "text": "part2"},
        ]},
    ])
    assert "part1 part2" in prompt


def test_claude_openai_chunk_shape():
    c = claude.openai_chunk("claude-sonnet-4-5", "hello")
    assert c["object"] == "chat.completion.chunk"
    assert c["choices"][0]["delta"]["content"] == "hello"
    assert c["choices"][0]["finish_reason"] is None


def test_claude_openai_chunk_finish():
    c = claude.openai_chunk("claude-sonnet-4-5", "", finish="stop")
    assert c["choices"][0]["finish_reason"] == "stop"
    assert c["choices"][0]["delta"] == {}


def test_chatgpt_build_input_text():
    instr, items = chatgpt._build_input([
        {"role": "system", "content": "be brief"},
        {"role": "user", "content": "hi"},
    ])
    assert instr == "be brief"
    assert items[0]["type"] == "message"
    assert items[0]["role"] == "user"
    assert items[0]["content"][0] == {"type": "input_text", "text": "hi"}


def test_chatgpt_build_input_image():
    _, items = chatgpt._build_input([
        {"role": "user", "content": [
            {"type": "text", "text": "what is this"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
        ]},
    ])
    parts = items[0]["content"]
    assert parts[0] == {"type": "input_text", "text": "what is this"}
    assert parts[1] == {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}


def test_chatgpt_build_input_tool_history():
    _, items = chatgpt._build_input([
        {"role": "assistant", "content": None, "tool_calls": [
            {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{}"}}]},
        {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
    ])
    assert items[0]["type"] == "function_call" and items[0]["call_id"] == "call_1"
    assert items[1]["type"] == "function_call_output" and items[1]["output"] == "sunny"


def test_chatgpt_tools_translation():
    out = chatgpt._tools([{"type": "function", "function": {
        "name": "f", "description": "d", "parameters": {"type": "object", "properties": {}}}}])
    assert out[0] == {"type": "function", "name": "f", "description": "d",
                      "parameters": {"type": "object", "properties": {}}}


def test_chatgpt_resolve_model():
    assert chatgpt._resolve_model("gpt-5.5") == "gpt-5.5"
    assert chatgpt._resolve_model("gpt-4o") == "gpt-5.5"
    assert chatgpt._resolve_model("anything-unknown") == "gpt-5.5"


def test_gemini_flatten_basic():
    prompt = gemini._flatten([
        {"role": "system", "content": "be brief"},
        {"role": "user", "content": "hi"},
    ])
    assert "System: be brief" in prompt
    assert "User: hi" in prompt


def test_gemini_flatten_list_content():
    prompt = gemini._flatten([
        {"role": "user", "content": [
            {"type": "text", "text": "p1 "},
            {"type": "text", "text": "p2"},
        ]}
    ])
    assert "User: p1 p2" in prompt


def test_gemini_model_header_default():
    assert "gemini-2.5-flash" in gemini._MODEL_HEADER
    assert gemini._MODEL_HEADER["gemini-2.5-pro"][1] == 1
