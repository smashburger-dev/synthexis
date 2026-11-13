"""Write the Ableton Live and Logic Pro stock labels into a known.tsv
without the label runs and torch that export_paramsense.py needs:

  python3 tools/mapping/merge_known.py [known.tsv ...]

(default plugin/assistant/completion/known.tsv). The file is parsed into the
table export_paramsense.py builds, checked to print back byte for byte,
its "ableton:" and "logic:" plugins are replaced by the stock tables
(stock.py), and it is printed
in the export's order (plugins sorted, parameters sorted). Every other line
stays as it was; a full `uv run tools/mapping/export_paramsense.py` writes
the same file.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import stock  # noqa: E402


def parse(text: str) -> dict:
    known, cur = {}, None
    for line in text.splitlines():
        # a parameter can be named "#000" too: a plugin line has no tab
        if line.startswith("#") and "\t" not in line:
            cur = known.setdefault(line[1:], {})
        else:
            n, c = line.split("\t")
            cur[n] = c
    return known


def render(known: dict) -> str:
    """export_paramsense.py's known.tsv layout."""
    lines = []
    for plug in sorted(known):
        lines.append("#" + plug)
        lines += [f"{n}\t{c}" for n, c in sorted(known[plug].items()) if "\t" not in n and "\n" not in n]
    return "\n".join(lines) + "\n"


def merge(text: str) -> str:
    known = parse(text)
    assert render(known) == text, "known.tsv is not in export order; rerun export_paramsense.py first"
    known = {p: v for p, v in known.items() if not p.startswith(stock.PREFIXES)}
    for plug, name, cls in stock.rows(stock.TABLES[1:]):
        assert plug.startswith(stock.PREFIXES), plug
        known.setdefault(plug.casefold().strip(), {})[name.casefold().strip()] = cls
    return render(known)


def main() -> None:
    root = Path(__file__).resolve().parents[2]
    for path in [Path(p) for p in sys.argv[1:]] or [root / "plugin" / "assistant" / "completion" / "known.tsv"]:
        old = path.read_text(encoding="utf-8")
        new = merge(old)
        path.write_text(new, encoding="utf-8")
        n = {p: sum(1 for line in new.splitlines() if line.startswith("#" + p)) for p in stock.PREFIXES}
        print(f"{path}: {old.count(chr(10))} -> {new.count(chr(10))} lines, {n}")


if __name__ == "__main__":
    main()
