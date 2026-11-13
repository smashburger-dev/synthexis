"""Parameter names of Ableton Live's stock devices as a remote script sees
them, taken from the parameter banks in Live's own MIDI Remote Scripts.

  python3 -I tools/mapping/ableton/lom_names.py <gluon checkout> <chiakibeats checkout> \
      > tools/mapping/ableton/lom_names.tsv

Live ships its control surface scripts as bytecode only; the two arguments
are decompiled mirrors on GitHub (pinned commits and versions in README.md).
The bank tables key devices by `Device.class_name` and list parameters by
`DeviceParameter.original_name`, the name a stock device reports. Live
checks those names against the real devices in C++ at startup, except the
pseudo-parameters in PARAMETERS_BLACKLIST_FOR_CPP_SANITY_CHECK (UI pages
like "Osc Select" that Push draws itself), which are dropped here.

The files are only parsed (ast), never imported or run. Arguments of
use/else_use/if_parameter/and_parameter and bare strings of a bank are
parameter names; with_name (Push display text), has_value (a value),
options and view descriptions are not.

"Device On" is added for every device: Live's v3 device component finds
the on switch of any device by original_name.startswith("Device On").

Output: class_name, name, sources (tags of the files that list the name).
Reimplemented for synthexis; no code from the mirrors is copied.
"""

import ast
import re
import sys
from pathlib import Path

NAME_CALLS = {"use", "else_use", "if_parameter", "and_parameter"}
SKIP_KEYS = {"OPTIONS_KEY", "VIEW_DESCRIPTION_KEY"}

# tag, mirror (0 gluon, 1 chiakibeats), file. Versions: README.md.
FILES = [
    ("v3-12.0", 0, "ableton/v3/control_surface/default_bank_definitions.py"),
    ("v2-12.0", 0, "ableton/v2/control_surface/default_bank_definitions.py"),
    ("push2-12.0", 0, "Push2/custom_bank_definitions.py"),
    ("generic-12.0", 0, "_Generic/Devices.py"),
    ("move-12.4", 0, "Move/custom_bank_definitions.py"),
    ("v3-12.1", 1, "ableton/v3/control_surface/default_bank_definitions.py"),
    ("move-12.1", 1, "Move/custom_bank_definitions.py"),
]

DEVICE_PY = "ableton/v3/control_surface/components/device.py"
DEVICE_ON = [("on-12.0", 0), ("on-12.1", 1)]


def names_in_call(node, out):
    """Parameter names in a use(...).if_parameter(...).else_use(...) chain."""
    while isinstance(node, ast.Call):
        f = node.func
        fname = f.id if isinstance(f, ast.Name) else f.attr if isinstance(f, ast.Attribute) else None
        if fname in NAME_CALLS:
            out += [a.value for a in node.args if isinstance(a, ast.Constant) and isinstance(a.value, str)]
        node = f.value if isinstance(f, ast.Attribute) else None


def bank_names(node, env, out):
    """Every parameter name inside a bank expression (IndexedDict of banks)."""
    if isinstance(node, ast.Name) and node.id in env:
        return bank_names(env[node.id], env, out)
    if isinstance(node, ast.Dict):
        for k, v in zip(node.keys, node.values):
            key = k.id if isinstance(k, ast.Name) else None
            if key in SKIP_KEYS:
                continue
            if key == "BANK_PARAMETERS_KEY":
                if isinstance(v, ast.Name) and v.id in env:
                    v = env[v.id]
                for e in getattr(v, "elts", []):
                    if isinstance(e, ast.Constant) and isinstance(e.value, str):
                        out.append(e.value)
                    else:
                        names_in_call(e, out)
            else:
                bank_names(v, env, out)
        return
    for child in ast.iter_child_nodes(node):
        bank_names(child, env, out)


def generic(tree, env):
    """_Generic/Devices.py: DEVICE_DICT maps class names to tuples of banks."""
    devs = {}
    for k, v in zip(env["DEVICE_DICT"].keys, env["DEVICE_DICT"].values):
        names = []
        for bank in env[v.id].elts:
            names += [e.value for e in env[bank.id].elts]
        devs[k.value] = names
    return devs


def parse(src: str):
    """Module tree; for a file the decompiler left broken, the statements
    that parse on their own (one per line in those files)."""
    try:
        return ast.parse(src).body
    except SyntaxError:
        body = []
        for line in src.splitlines():
            try:
                body += ast.parse(line).body
            except SyntaxError:
                pass
        return body


def device_banks(path: Path):
    body = parse(path.read_text(encoding="utf-8"))
    env, devs, black = {}, {}, {}
    for st in body:
        if not isinstance(st, ast.Assign) or len(st.targets) != 1:
            continue
        t, v = st.targets[0], st.value
        if isinstance(t, ast.Name):
            env[t.id] = v
            if t.id.endswith("BANK_DEFINITIONS") and isinstance(v, ast.Dict):
                for k, b in zip(v.keys, v.values):
                    if isinstance(k, ast.Constant):
                        devs[k.value] = b
            if t.id == "PARAMETERS_BLACKLIST_FOR_CPP_SANITY_CHECK":
                for k, b in zip(v.keys, v.values):
                    # some entries are a quoted string instead of a tuple
                    vals = [e.value for e in getattr(b, "elts", [b]) if isinstance(e, ast.Constant)]
                    black[k.value] = {x.strip('"') for x in vals}
        elif isinstance(t, ast.Subscript) and isinstance(t.value, ast.Name) and t.value.id.endswith("BANK_DEFINITIONS"):
            if isinstance(t.slice, ast.Constant):
                devs[t.slice.value] = v
    if "DEVICE_DICT" in env:
        return generic(body, env), black
    out = {}
    for cls, node in devs.items():
        names = []
        bank_names(node, env, names)
        out[cls] = names
    return out, black


def main() -> None:
    mirrors = [Path(p) for p in sys.argv[1:3]]
    seen, black = {}, {}
    for tag, m, rel in FILES:
        devs, bl = device_banks(mirrors[m] / rel)
        for cls, names in bl.items():
            black.setdefault(cls, set()).update(names)
        for cls, names in devs.items():
            for n in names:
                if n.strip() and not re.search(r"[{}]", n):
                    seen.setdefault((cls, n), []).append(tag)
    # every device's on switch: Live's scripts find it by
    # original_name.startswith("Device On") in device.parameters
    for tag, m in DEVICE_ON:
        src = (mirrors[m] / DEVICE_PY).read_text(encoding="utf-8")
        assert re.search(r"original_name\.startswith\(['\"]Device On['\"]\)", src), tag
        for cls in {c for c, _ in seen}:
            seen.setdefault((cls, "Device On"), []).append(tag)
    print("class_name\tname\tsources")
    for (cls, n), tags in sorted(seen.items(), key=lambda x: (x[0][0].casefold(), x[0][1].casefold())):
        if n in black.get(cls, ()):
            continue
        print(f"{cls}\t{n}\t{','.join(dict.fromkeys(tags))}")


if __name__ == "__main__":
    main()
