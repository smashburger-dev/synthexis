# /// script
# requires-python = ">=3.11"
# dependencies = ["mido", "python-rtmidi"]
# ///
"""Send the CC probes for the FL Studio 'synthexis bridge probe' script.

Usage:
  uv run send_cc.py ports               # list MIDI outputs
  uv run send_cc.py dump                # CC 21: dump plugin params
  uv run send_cc.py set-index N         # CC 22: select param index
  uv run send_cc.py sweep               # CC 20: 0->127->0 over 4 s at 50 Hz
  uv run send_cc.py sweep-rec           # CC 23: same sweep, recording path
  add --port NAME to pick a port (default: first port containing 'IAC')
"""
import sys, time, mido

def pick_port(name=None):
    outs = mido.get_output_names()
    if name:
        for o in outs:
            if name.lower() in o.lower():
                return o
        print("no port containing", name); print("available:", outs); sys.exit(2)
    for o in outs:
        if "iac" in o.lower():
            return o
    print("no IAC port; available:", outs); sys.exit(2)

def main():
    args = sys.argv[1:]
    port = None
    if "--port" in args:
        i = args.index("--port"); port = args[i + 1]; del args[i:i + 2]
    cmd = args[0] if args else "ports"
    if cmd == "ports":
        print("\n".join(mido.get_output_names())); return
    out = mido.open_output(pick_port(port))
    if cmd == "dump":
        out.send(mido.Message("control_change", control=21, value=0))
    elif cmd == "set-index":
        out.send(mido.Message("control_change", control=22, value=int(args[1])))
    elif cmd in ("sweep", "sweep-rec"):
        cc = 20 if cmd == "sweep" else 23
        # 4 s at 50 Hz: up 2 s, down 2 s
        for v in list(range(0, 128, 5)) + list(range(128, 0, -5)):
            out.send(mido.Message("control_change", control=cc, value=min(v, 127)))
            time.sleep(0.02)
    else:
        print(__doc__); sys.exit(2)

main()
