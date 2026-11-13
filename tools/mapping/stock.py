"""Hand-labelled stock plugins of the DAWs, for known.tsv.

  tools/mapping/fl/stock_labels.tsv       FL Studio stock effects, keyed by
                                          the plugin name FL reports
  tools/mapping/ableton/stock_labels.tsv  Ableton Live stock devices, keyed
                                          "ableton:<Device.class_name>"
  tools/mapping/logic/stock_labels.tsv    Logic Pro's own plug-ins, keyed
                                          "logic:<plug-in name>"

Both TSVs have a header with at least the columns plugin, name, class.
No dependencies, so export_paramsense.py and merge_known.py share it.
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TABLES = [ROOT / "tools" / "mapping" / "fl" / "stock_labels.tsv",
          ROOT / "tools" / "mapping" / "ableton" / "stock_labels.tsv",
          ROOT / "tools" / "mapping" / "logic" / "stock_labels.tsv"]
# key prefixes of the DAW tables whose plugins are not plug-in names
PREFIXES = ("ableton:", "logic:")


def rows(tables=TABLES):
    """(plugin, parameter name, class) of every hand-labelled stock parameter."""
    for path in tables:
        lines = path.read_text(encoding="utf-8").splitlines()
        head = lines[0].split("\t")
        p, n, c = head.index("plugin"), head.index("name"), head.index("class")
        for line in lines[1:]:
            f = line.split("\t")
            assert len(f) == len(head), (path.name, line)
            yield f[p], f[n], f[c]
