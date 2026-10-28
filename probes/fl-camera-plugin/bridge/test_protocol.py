#!/usr/bin/env python3
"""Unit tests for the SysEx framing (sysexcodec) and the device script's
codec parity. Run: python3 test_protocol.py"""

import importlib.util
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import sysexcodec  # noqa: E402


def test_roundtrip():
    for obj in [
        {"id": 1, "cmd": "ping"},
        {"id": 42, "cmd": "set", "ch": 0, "idx": 3, "value": 0.5},
        {"id": 999, "cmd": "channels", "extra": "a" * 500},
    ]:
        frame = sysexcodec.encode(obj)
        assert frame[0] == 0xF0 and frame[1] == 0x7D and frame[-1] == 0xF7
        assert all(b < 0x80 for b in frame[1:-1])
        assert sysexcodec.decode(frame) == obj


def test_ascii_only():
    # json.dumps escapes non-ASCII to \uXXXX, so encoded frames are always
    # 7-bit safe; unicode still round-trips correctly.
    obj = {"cmd": "x", "v": "über"}
    frame = sysexcodec.encode(obj)
    assert all(b < 0x80 for b in frame[2:-1])  # payload bytes only
    assert sysexcodec.decode(frame) == obj


def test_oversize():
    try:
        sysexcodec.encode({"cmd": "x", "v": "a" * 5000})
    except ValueError:
        pass
    else:
        raise AssertionError("oversize payload accepted")


def test_feed_fragmented():
    frame = sysexcodec.encode({"id": 7, "cmd": "get", "ch": 0, "idx": 2})
    buf = bytearray()
    out = None
    for i in range(0, len(frame), 3):
        r = sysexcodec.feed(buf, frame[i:i + 3])
        if r is not None:
            out = r
    assert out == frame
    assert sysexcodec.decode(out)["id"] == 7


def test_feed_garbage_and_multiple():
    f1 = sysexcodec.encode({"id": 1})
    f2 = sysexcodec.encode({"id": 2})
    buf = bytearray(b"\x11\x22\xf0\x7c\x66")
    out = []
    r = sysexcodec.feed(buf, f1 + f2)
    while r is not None:
        out.append(r)
        r = sysexcodec.feed(buf, b"")
    assert out == [f1, f2]


def test_device_codec_parity():
    """The device script's decoder contract: F0 7D <json> F7 round-trips."""
    src = (Path(__file__).parent / "device_synthexis_bridge.py").read_text()
    assert "data[start + 1] != 0x7D" in src  # magic byte matches codec
    obj = {"id": 5, "cmd": "show", "ch": 1, "on": 1}
    frame = sysexcodec.encode(obj)
    data = bytes(frame)
    start = data.index(0xF0)
    assert data[start + 1] == 0x7D
    end = data.index(0xF7, start + 2)
    import json as _j
    assert _j.loads(data[start + 2:end].decode("ascii")) == obj


if __name__ == "__main__":
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for t in tests:
        t()
        print(f"PASS {t.__name__}")
    print(f"{len(tests)} tests passed")
