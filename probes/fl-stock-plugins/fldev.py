# /// script
# requires-python = ">=3.11"
# dependencies = ["python-rtmidi==1.5.8"]
# ///
"""Shell access to FL Studio's script API for probes, through
device_synthexis_dev.py (this folder).

  uv run probes/fl-stock-plugins/fldev.py serve &      # owns the MIDI ports
  uv run probes/fl-stock-plugins/fldev.py call 'mixer.trackCount()' [--main]
  uv run probes/fl-stock-plugins/fldev.py file code.py [--main]

serve opens a virtual MIDI source and destination named "sxdev",
answers FL's device inquiry with 7D 53 58 44 56 (FL then loads the dev
script), and relays JSON lines from a unix socket to FL and back, matched by
"id". call/file send one piece of code and print the reply as JSON.
"""

import itertools
import json
import os
import socket
import sys
import threading
import time

SOCK = "/tmp/synthexis-fldev.sock"
NAME = "sxdev"  # no "synthexis" prefix, see device_synthexis_dev.py
IDENTITY = [0x7D, 0x53, 0x58, 0x44, 0x56, 0, 0, 0, 0]
TIMEOUT = float(os.environ.get("FLDEV_TIMEOUT", "20"))


def serve() -> None:
    import rtmidi

    out = rtmidi.MidiOut()
    out.open_virtual_port(NAME)
    inp = rtmidi.MidiIn(queue_size_limit=4096)
    inp.ignore_types(sysex=False, timing=True, active_sense=True)
    inp.open_virtual_port(NAME)
    waiting: dict[int, list] = {}
    cond = threading.Condition()

    def on_msg(event, _data):
        msg, _ = event
        if msg[:2] == [0xF0, 0x7E] and msg[3:5] == [0x06, 0x01]:
            out.send_message([0xF0, 0x7E, 0x7F, 0x06, 0x02] + IDENTITY + [0xF7])
            print("fldev: FL asked who we are", flush=True)
            return
        if msg[:2] != [0xF0, 0x7D]:
            return
        try:
            r = json.loads(bytes(msg[2:-1]).decode("ascii"))
        except Exception as e:
            print("fldev: bad frame", e, len(msg), flush=True)
            return
        if "ev" in r:
            if r.get("ev") == "sxdev":
                print("fldev: dev script up", flush=True)
            return
        rid = r.get("id")
        if not isinstance(rid, int) or rid <= 1_000_000:
            return  # the bridge's reply to the plugin, seen through a shared port
        with cond:
            waiting[rid] = [r]
            cond.notify_all()

    inp.set_callback(on_msg)
    # FL may give this device the same port number as the synthexis bridge,
    # so both scripts' replies reach both clients; ids from 1,000,001 on
    # never meet the plugin's own request ids, and unknown ids are dropped.
    ids = itertools.count(1_000_001)
    if os.path.exists(SOCK):
        os.unlink(SOCK)
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(SOCK)
    srv.listen(4)
    print("fldev: serving on", SOCK, flush=True)

    def handle(conn):
        with conn, conn.makefile("rw") as f:
            for line in f:
                req = json.loads(line)
                rid = next(ids)
                req["id"] = rid
                data = json.dumps(req, separators=(",", ":")).encode("ascii")
                out.send_message([0xF0, 0x7D] + list(data) + [0xF7])
                deadline = time.time() + TIMEOUT
                with cond:
                    while rid not in waiting and time.time() < deadline:
                        cond.wait(deadline - time.time())
                    rep = waiting.pop(rid, [{"id": rid, "error": "timeout"}])[0]
                f.write(json.dumps(rep) + "\n")
                f.flush()

    while True:
        conn, _ = srv.accept()
        threading.Thread(target=handle, args=(conn,), daemon=True).start()


def call(code: str, main: bool = False) -> dict:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCK)
    with s, s.makefile("rw") as f:
        f.write(json.dumps({"py": code, "main": main}) + "\n")
        f.flush()
        return json.loads(f.readline())


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[0] == "serve":
        serve()
    else:
        code = open(a[1]).read() if a[0] == "file" else a[1]
        r = call(code, "--main" in a)
        if r.get("out"):
            sys.stdout.write(r["out"])
        print(json.dumps({k: v for k, v in r.items() if k != "out"}, ensure_ascii=False))
