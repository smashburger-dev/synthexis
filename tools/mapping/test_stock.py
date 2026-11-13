# /// script
# requires-python = ">=3.11"
# ///
"""Checks of the Ableton Live and Logic Pro stock labels and their
known.tsv export (no network, no torch, no data/):

  uv run --with pytest --with pyyaml pytest tools/mapping/test_stock.py
"""

import re
import subprocess
import sys
from pathlib import Path

import yaml

MAP = Path(__file__).resolve().parent
HERE = MAP / "ableton"
LOGIC = MAP / "logic"
ROOT = MAP.parents[1]
KNOWN = ROOT / "plugin" / "assistant" / "completion" / "known.tsv"
sys.path.insert(0, str(MAP))
import merge_known  # noqa: E402
import stock  # noqa: E402

ONTO = {c["id"] for c in yaml.safe_load((MAP / "ontology.yaml").read_text())["taxonomy"]}
MIXES = {"fx.reverb.mix", "fx.delay.mix", "fx.mod.mix", "fx.mix"}


def table(name, d=HERE):
    lines = (d / name).read_text(encoding="utf-8").splitlines()
    head = lines[0].split("\t")
    return [dict(zip(head, line.split("\t"))) for line in lines[1:]]


LABELS = table("stock_labels.tsv")
LOGIC_LABELS = table("stock_labels.tsv", LOGIC)


def label(cls, name):
    return next(r["class"] for r in LABELS if r["class_name"] == cls and r["name"] == name)


def test_tsvs_are_what_the_scripts_write():
    for d in (HERE, LOGIC):
        out = subprocess.run([sys.executable, str(d / "stock_labels.py")], capture_output=True, text=True, check=True)
        assert out.stdout == (d / "stock_labels.tsv").read_text(encoding="utf-8"), d.name


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
    assert len([p for p in plugins if p.startswith("logic:")]) == len({r["plugin"] for r in LOGIC_LABELS})
    # display names would collide ("Frequency Shifter" is FL's); class names and prefixed keys do not
    others = {p for p in plugins if not p.startswith(stock.PREFIXES)}
    assert not {r["class_name"].casefold() for r in LABELS} & others


def test_known_tsv_is_current_and_other_plugins_unchanged():
    text = KNOWN.read_text(encoding="utf-8")
    assert merge_known.merge(text) == text
    known = merge_known.parse(text)
    without = {p: v for p, v in known.items() if not p.startswith(stock.PREFIXES)}
    assert merge_known.merge(merge_known.render(without)) == text
    assert sum(len(v) for p, v in known.items() if p.startswith("ableton:")) == len(LABELS)
    assert sum(len(v) for p, v in known.items() if p.startswith("logic:")) == len(LOGIC_LABELS)


def test_stock_rows_reads_every_table():
    fl, live, logic = (list(stock.rows([t])) for t in stock.TABLES)
    assert len(fl) == 680 and fl[0][0] == "Effector"
    assert len(live) == len(LABELS) and len(logic) == len(LOGIC_LABELS)


def test_logic_labels():
    names = {(r["plugin"], r["name"]) for r in table("manual_names.tsv", LOGIC)}
    got = [(r["plugin_name"], r["name"]) for r in LOGIC_LABELS]
    assert len(got) == len(set(got)) and set(got) == names
    seen = set()
    for r in LOGIC_LABELS:
        assert r["plugin"] == "logic:" + r["plugin_name"] and r["class"] in ONTO and r["status"] == "UI label"
        key = (r["plugin"].casefold(), r["name"].casefold().strip())
        assert key not in seen and not r["name"].startswith("#") and "\t" not in r["name"]
        seen.add(key)
        if re.search(r"\bdry\b", r["name"], re.I) and not re.search(r"wet", r["name"], re.I):
            assert r["class"] not in MIXES, r
    cls = {(r["plugin_name"], r["name"]): r["class"] for r in LOGIC_LABELS}
    assert cls[("ChromaVerb", "Dry/Wet")] == "fx.reverb.mix"
    assert cls[("Tape Delay", "Feedback")] == "fx.delay.feedback"
    assert cls[("Compressor", "Threshold")] == "fx.comp.amount"
    assert cls[("Retro Synth", "Semitones")] == "osc.pitch"
