from app.providers import for_model, list_models
from app.providers import claude, chatgpt, gemini


def test_for_model_routing_claude():
    assert for_model("claude-sonnet-4-5") is claude
    assert for_model("claude-opus-4-5") is claude
    assert for_model("claude-3-5-haiku-20241022") is claude


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


def test_chatgpt_to_parts_string():
    parts = chatgpt._to_parts([{"role": "user", "content": "hi"}])
    assert parts[0]["author"]["role"] == "user"
    assert parts[0]["content"]["parts"] == ["hi"]
    assert parts[0]["content"]["content_type"] == "text"


def test_chatgpt_to_parts_list_content():
    parts = chatgpt._to_parts([
        {"role": "user", "content": [
            {"type": "text", "text": "hi"},
            {"type": "text", "text": " there"},
        ]}
    ])
    assert parts[0]["content"]["parts"] == ["hi there"]


def test_chatgpt_to_parts_system_role():
    parts = chatgpt._to_parts([{"role": "system", "content": "be brief"}])
    assert parts[0]["author"]["role"] == "system"


def test_chatgpt_device_id_from_cookie():
    sess = {"cookies": [{"name": "oai-did", "value": "device-123"}]}
    assert chatgpt._device_id(sess) == "device-123"


def test_chatgpt_device_id_fallback():
    did = chatgpt._device_id({"cookies": []})
    assert len(did) > 0


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
