#!/usr/bin/env python3
"""flb — client for the synthexis FL bridge daemon (flbridged).

  flb.py call '<json>'            send one command, print the reply
  from flb import call; call("transport", op="status")

Socket: /tmp/synthexis-flbridge.sock, JSON lines, one command per line,
one reply line per command.
"""

import json
import socket
import sys

SOCK = "/tmp/synthexis-flbridge.sock"


def call(cmd, **args):
    """Send one command dict to the bridge; return the reply dict."""
    req = {"cmd": cmd}
    req.update(args)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(SOCK)
        f = s.makefile("rw")
        f.write(json.dumps(req, separators=(",", ":")) + "\n")
        f.flush()
        line = f.readline()
    return json.loads(line)


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "call":
        print(json.dumps(call(**json.loads(sys.argv[2]))))
    else:
        print(__doc__.strip())
        sys.exit(2)
