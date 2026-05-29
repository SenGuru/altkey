from app.harvester import cookie_header, _LOGIN_URLS, _COOKIE_KEYS


def test_login_urls_present():
    # Gemini parked; harvester itself will be removed in Task 0.5
    assert {"claude", "chatgpt"} <= set(_LOGIN_URLS)


def test_cookie_keys_present():
    assert "sessionKey" in _COOKIE_KEYS["claude"]
    assert "__Secure-next-auth.session-token" in _COOKIE_KEYS["chatgpt"]
    # Gemini parked: assert "__Secure-1PSID" in _COOKIE_KEYS["gemini"]


def test_cookie_header_basic():
    sess = {"cookies": [
        {"name": "a", "value": "1", "domain": ".claude.ai"},
        {"name": "b", "value": "2", "domain": "openai.com"},
    ]}
    hdr = cookie_header(sess, ("claude.ai",))
    assert "a=1" in hdr
    assert "b=2" not in hdr


def test_cookie_header_dotted_domain_match():
    sess = {"cookies": [{"name": "x", "value": "y", "domain": ".sub.claude.ai"}]}
    hdr = cookie_header(sess, ("claude.ai",))
    assert hdr == "x=y"


def test_cookie_header_empty():
    assert cookie_header({"cookies": []}, ("claude.ai",)) == ""
    assert cookie_header({}, ("claude.ai",)) == ""
