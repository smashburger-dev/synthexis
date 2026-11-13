# /// script
# requires-python = ">=3.11"
# ///
"""Checks of the Ableton Live stock labels and their known.tsv export
(no network, no torch, no data/):

  uv run --with pytest --with pyyaml pytest tools/mapping/ableton/test_stock.py
"""

import re
import subprocess
import sys
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
KNOWN = ROOT / "plugin" / "assistant" / "completion" / "known.tsv"
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import merge_known  # noqa: E402
import stock  # noqa: E402

ONTO = {c["id"] for c in yaml.safe_load((HERE.parent / "ontology.yaml").read_text())["taxonomy"]}
MIXES = {"fx.reverb.mix", "fx.delay.mix", "fx.mod.mix", "fx.mix"}


def table(name):
    lines = (HERE / name).read_text(encoding="utf-8").splitlines()
    head = lines[0].split("\t")
    return [dict(zip(head, line.split("\t"))) for line in lines[1:]]


LABELS = table("stock_labels.tsv")


def label(cls, name):
    return next(r["class"] for r in LABELS if r["class_name"] == cls and r["name"] == name)


def test_tsv_is_what_the_script_writes():
    out = subprocess.run([sys.executable, str(HERE / "stock_labels.py")], capture_output=True, text=True, check=True)
    assert out.stdout == (HERE / "stock_labels.tsv").read_text(encoding="utf-8")


def test_every_lom_name_is_labelled_once_with_an_ontology_class():
    names = {(r["class_name"], r["name"]) for r in table("lom_names.tsv")}
    got = [(r["class_name"], r["name"]) for r in LABELS]
    assert len(got) == len(set(got))
    assert set(got) == {k for k in names if k[0] != "ProxyAudioEffectDevice"}
    assert {r["class"] for r in LABELS} <= ONTO
    assert all(r["status"] in ("12.4", "12.1", "12.0", "UNVERIFIED in 12.4") for r in LABELS)


def test_keys_are_prefixed_and_names_unique_per_device():
    seen = {}
    for r in LABELS:
        assert r["plugin"] == "ableton:" + r["class_name"]
        key = (r["plugin"].casefold(), r["name"].casefold().strip())
        assert key not in seen, key
        seen[key] = r["class"]
        assert not r["name"].startswith("#") and "\t" not in r["name"]


def test_a_pure_dry_level_never_takes_a_mix_class():
    for r in LABELS:
        if re.search(r"\bdry\b", r["name"], re.I) and not re.search(r"wet", r["name"], re.I):
            assert r["class"] not in MIXES, r


def test_anchor_parameters():
    assert label("Reverb", "Dry/Wet") == "fx.reverb.mix"
    assert label("Reverb", "Decay Time") == "fx.reverb.decay"
    assert label("Delay", "Feedback") == "fx.delay.feedback"
    assert label("AutoFilter2", "Frequency") == "filter.cutoff"
    assert label("Eq8", "1 Gain A") == "fx.eq.gain"
    assert label("Compressor2", "Threshold") == "fx.comp.amount"
    assert label("InstrumentVector", "Osc 1 Pos") == "osc.shape"
    assert label("Operator", "Ae Attack") == "env.attack"
    assert label("StereoGain", "Gain") == "unsafe"
    assert label("AudioEffectGroupDevice", "Macro 1") == "macro"
    # one cutoff per device where low- and high-pass would tie
    assert label("Drift", "HP Freq") == "other"


def test_no_live_key_collides_with_a_plugin_of_the_table():
    text = KNOWN.read_text(encoding="utf-8")
    plugins = [line[1:] for line in text.splitlines() if line.startswith("#") and "\t" not in line]
    live = [p for p in plugins if p.startswith("ableton:")]
    assert len(live) == len({r["class_name"] for r in LABELS})
    assert len(plugins) == len(set(plugins))
    # display names would collide ("Frequency Shifter" is FL's); class names and prefixed keys do not
    others = {p for p in plugins if not p.startswith("ableton:")}
    assert not {r["class_name"].casefold() for r in LABELS} & others


def test_known_tsv_is_current_and_other_plugins_unchanged():
    text = KNOWN.read_text(encoding="utf-8")
    assert merge_known.merge(text) == text
    known = merge_known.parse(text)
    without = {p: v for p, v in known.items() if not p.startswith("ableton:")}
    assert merge_known.merge(merge_known.render(without)) == text
    assert sum(len(v) for p, v in known.items() if p.startswith("ableton:")) == len(LABELS)


def test_stock_rows_reads_both_tables():
    fl, live = (list(stock.rows([t])) for t in stock.TABLES)
    assert len(fl) == 680 and fl[0][0] == "Effector"
    assert len(live) == len(LABELS)
