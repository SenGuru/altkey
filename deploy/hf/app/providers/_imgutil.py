import base64
import binascii
import struct


def extract_images(messages: list[dict]) -> list[tuple[bytes, str]]:
    """Return [(bytes, mime)] for every image_url (data: URL) part."""
    out = []
    for m in messages:
        content = m.get("content")
        if not isinstance(content, list):
            continue
        for part in content:
            if not isinstance(part, dict) or part.get("type") != "image_url":
                continue
            url = (part.get("image_url") or {}).get("url", "")
            if url.startswith("data:"):
                try:
                    header, b64 = url.split(",", 1)
                    mime = header[5:].split(";")[0] or "image/png"
                    out.append((base64.b64decode(b64), mime))
                except (ValueError, binascii.Error):
                    continue
    return out


def image_size(data: bytes, mime: str) -> tuple[int, int]:
    """Best-effort (width, height) from PNG/JPEG/GIF/WEBP bytes. Falls back
    to (1024, 1024) if it can't parse."""
    try:
        if data[:8] == b"\x89PNG\r\n\x1a\n":
            w, h = struct.unpack(">II", data[16:24])
            return int(w), int(h)
        if data[:3] == b"\xff\xd8\xff":  # JPEG
            i = 2
            n = len(data)
            while i < n:
                if data[i] != 0xFF:
                    i += 1
                    continue
                marker = data[i + 1]
                if 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
                    h, w = struct.unpack(">HH", data[i + 5 : i + 9])
                    return int(w), int(h)
                seg_len = struct.unpack(">H", data[i + 2 : i + 4])[0]
                i += 2 + seg_len
        if data[:6] in (b"GIF87a", b"GIF89a"):
            w, h = struct.unpack("<HH", data[6:10])
            return int(w), int(h)
        if data[:4] == b"RIFF" and data[8:12] == b"WEBP":
            # VP8X / VP8 / VP8L — handle the common VP8X case.
            if data[12:16] == b"VP8X":
                w = 1 + int.from_bytes(data[24:27], "little")
                h = 1 + int.from_bytes(data[27:30], "little")
                return w, h
    except (struct.error, IndexError):
        pass
    return 1024, 1024
