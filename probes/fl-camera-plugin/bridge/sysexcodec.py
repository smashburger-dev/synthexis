"""Shared SysEx framing for the synthexis FL bridge.

Wire format: F0 7D <7-bit-safe UTF-8 JSON> F7
(0x7D = non-commercial manufacturer ID; JSON must be ASCII-only.)

Used by flb.py / h2_run.py; mirrors the device script's codec.
"""

import json

MAGIC = 0x7D
MAX_PAYLOAD = 4096


def encode(obj):
    """Encode a command dict as a sysex frame (bytes)."""
    data = json.dumps(obj, separators=(",", ":")).encode("ascii")
    if len(data) > MAX_PAYLOAD:
        raise ValueError(f"sysex payload {len(data)} > {MAX_PAYLOAD}")
    for b in data:
        if b > 0x7F:
            raise ValueError(f"non-7-bit byte {b:#x} in payload")
    return bytes([0xF0, MAGIC]) + data + bytes([0xF7])


def decode(frame):
    """Decode a sysex frame (bytes or list of ints) to a dict, or None."""
    data = bytes(frame)
    try:
        start = data.index(0xF0)
        if start + 1 >= len(data) or data[start + 1] != MAGIC:
            return None
        end = data.index(0xF7, start + 2)
        return json.loads(data[start + 2:end].decode("ascii"))
    except (ValueError, UnicodeDecodeError):
        return None


def feed(buffer, chunk):
    """Accumulate sysex bytes split across MIDI packets.

    buffer: bytearray holding partial data. Returns a completed frame
    (bytes) or None; clears the buffer on completion/resync.
    """
    buffer += bytes(chunk)
    while True:
        try:
            start = buffer.index(0xF0)
        except ValueError:
            buffer.clear()
            return None
        if start > 0:
            del buffer[:start]
        if len(buffer) < 2:
            return None
        if buffer[1] != MAGIC:
            del buffer[0]
            continue
        try:
            end = buffer.index(0xF7, 2)
        except ValueError:
            if len(buffer) > MAX_PAYLOAD + 4:
                buffer.clear()
            return None
        frame = bytes(buffer[: end + 1])
        del buffer[: end + 1]
        return frame
