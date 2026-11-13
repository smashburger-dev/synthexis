# /// script
# requires-python = ">=3.11"
# ///
"""Unit tests for label.py / commands.py / edits.py / fl_param_counts.py
(no network).

  uv run --with pytest --with httpx --with pyyaml pytest \
      tools/mapping/test_mapping.py
"""

import json
import random
import re
import sys
from pathlib import Path

import pytest
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
import commands  # noqa: E402
import edits as ed  # noqa: E402
import fl_param_counts  # noqa: E402
import label  # noqa: E402
import spark  # noqa: E402

HERE = Path(__file__).resolve().parent
ONTO = yaml.safe_load((HERE / "ontology.yaml").read_text())
CFG = yaml.safe_load((HERE / "edits.yaml").read_text())


def param(name, index=0, unit="generic", flags=None, group=None,
          samples=None, value_strings=None, default_string="50%"):
    return {"plugin_id": "M/P", "index": index, "name": name,
            "group": group or [], "unit": unit, "unit_name": "",
            "min": 0.0, "max": 1.0, "default": 0.5,
            "flags": flags or ["readable", "writable"],
            "value_strings": value_strings,
            "samples": samples or [[i / 10, f"{i * 10}%"]
                                   for i in range(11)],
            "default_string": default_string}


# --- label.py -------------------------------------------------------------


def test_rule_labels():
    assert label.rule_label(param("#014")) == ("macro", 1, 0, 1)
    assert label.rule_label(param("Param 12")) == ("macro", 1, 0, 1)
    assert label.rule_label(param("p 3")) == ("macro", 1, 0, 1)
    assert label.rule_label(param("-")) == "skip"
    assert label.rule_label(param("reserved")) == "skip"
    assert label.rule_label(param("MIDI CC 74")) == ("system", 1, 0, 1)
    assert label.rule_label(param("CC 16")) == ("system", 1, 0, 1)
    assert label.rule_label(param("Meter",
                                  flags=["meter_read_only"])) \
        == ("system", 1, 0, 1)
    assert label.rule_label(param("Cutoff", flags=["readable"])) \
        == ("system", 1, 0, 1)
    assert label.rule_label(param("Cutoff")) is None


def test_render_values():
    assert label.render_values(param("x", value_strings=["Off", "On"])) \
        == "switch: Off / On"
    vs = [f"v{i}" for i in range(15)]
    out = label.render_values(param("x", value_strings=vs))
    assert out.startswith("list: v0 / v1") and "(+3 more)" in out
    assert label.render_values(param("x", unit="boolean",
                                     samples=[[0, "off"], [1, "on"]])) \
        == "switch: off / on"
    assert label.render_values(param("x")) == "0% .. 20% .. 50% .. 80% .. 100%"


def test_render_line_group():
    assert label.render_line(3, param("Freq")).startswith("3 | Freq |")
    assert label.render_line(0, param("Freq", group=["a", "b"])) \
        .startswith("0 | a > b > Freq |")
    assert "default 50%" in label.render_line(0, param("Freq"))


def test_template_and_plugin_dedup():
    plugins = {
        "M/One": {"name": "One", "manufacturer_name": "M",
                  "type_name": "Effect", "tags": []},
        "M/OneStereo": {"name": "OneStereo", "manufacturer_name": "M",
                        "type_name": "Effect", "tags": []},
    }
    plist = [param("Cutoff", 0), param("Band 1 Gain", 1),
             param("Band 2 Gain", 2)]
    params = {"M/One": plist, "M/OneStereo":
              [dict(p, plugin_id="M/OneStereo") for p in plist]}
    label_of, reps = label.build_plugin_work(plugins, params)
    # Band 2 Gain copies the Band # Gain template -> only 2 reps
    assert len(reps["M/One"]) == 2
    # stereo variant: identical names -> every param is a plugin copy
    assert reps.get("M/OneStereo", []) == []
    assert all(v[0] == "copy" for v in label_of.values()
               if isinstance(v, tuple))


def test_label_parse():
    label.JOB_META["j0"] = {"n": 3, "params": {}}
    parse = label.make_parse({"filter.cutoff", "fx.mix", "system"})
    ok = parse(spark.Job("j0", [], 0),
               '[0,"filter.cutoff",1,0.05,1]\n'
               '[1,"fx.mix",-1,0,0.9]\n[2,"system",1,0,1]')
    assert ok["labels"][2] == ["system", 1, 0.0, 1.0]
    assert ok["missing"] == []
    # partial: one of three -> <50% -> ParseError
    with pytest.raises(spark.ParseError):
        parse(spark.Job("j0", [], 0), '[0,"filter.cutoff",1,0,1]')
    # garbage text -> ParseError
    with pytest.raises(spark.ParseError):
        parse(spark.Job("j0", [], 0), "no json here")
    # bad class and out-of-range index are skipped, missing reported
    res = parse(spark.Job("j0", [], 0),
                '[0,"nope",1,0,1]\n[1,"fx.mix",1,0,1]\n'
                '[2,"system",1,0,1]\n[9,"system",1,0,1]')
    assert res["missing"] == [0]
    # bad range rejected -> counted as missing
    res = parse(spark.Job("j0", [], 0),
                '[0,"filter.cutoff",1,0,1]\n[1,"fx.mix",1,0.5,0.4]\n'
                '[2,"system",1,0,1]')
    assert res["missing"] == [1]


# --- commands.py ----------------------------------------------------------


def make_edit(intent="map"):
    return {
        "id": "e000001", "lang": "en",
        "variants": [{"register": "casual", "noise": "clean"}],
        "intent": intent,
        "slots": {
            "s1": {"kind": "source", "value": "hand.fist"},
            "t1": {"kind": "target", "target": "param",
                   "value": "fx.reverb.mix"},
            "k1": {"kind": "track", "value": "pad"},
            "i1": {"kind": "invert", "value": True},
        },
        "routes": [{"src": ["s1"], "tgt": ["t1"], "scope": ["k1"],
                    "mods": ["i1"]}],
        "payload": {}}


def test_validate_variant_ok():
    e = make_edit()
    ok, reason, s = commands.validate_variant(
        e, 1, "[[my fist|s1]] controls the [[reverb|t1]] "
              "on the [[pad|k1]]")
    assert ok and s["text"] == \
        "my fist controls the reverb on the pad"
    assert [sp["slot"] for sp in s["spans"]] == ["s1", "t1", "k1"]
    assert s["spans"][0] == {"start": 0, "end": 7, "slot": "s1",
                             "text": "my fist"}
    # i1 optional
    ok2, _, _ = commands.validate_variant(
        e, 1, "[[fist|s1]] -> [[verb|t1]] on [[pad|k1]] "
              "[[inverted|i1]]")
    assert ok2


def test_validate_variant_failures():
    e = make_edit()
    ok, reason, _ = commands.validate_variant(
        e, 1, "[[fist|s1]] controls [[reverb|t1]]")
    assert not ok and reason == "missing k1"
    ok, reason, _ = commands.validate_variant(
        e, 1, "[[fist|s1]] -> [[reverb|t1]] [[pad|k1]] [[x|zz9]]")
    assert not ok and reason.startswith("unknown slot")
    ok, reason, _ = commands.validate_variant(
        e, 1, "[[fist|s1]] -> [[reverb|t1]] [[pad|k1]] [[oops]]")
    assert not ok and reason == "leftover mark"
    ok, reason, _ = commands.validate_variant(
        e, 1, "[[fist|s1]] [[x|i1]] [[y|i1]] [[reverb|t1]] [[pad|k1]]")
    assert not ok and reason == "i1 marked twice"
    none_edit = {"id": "e9", "lang": "en", "intent": "none",
                 "variants": [{"register": "casual", "noise": "clean"}],
                 "slots": {}, "routes": [], "payload": {}}
    ok, reason, _ = commands.validate_variant(
        none_edit, 1, "this plugin is [[great|s1]]")
    assert not ok and reason == "marks on none intent"


def test_commands_parse_threshold():
    commands.JOB_META["j0"] = {"edits": [make_edit()]}
    parse = commands.make_parse()
    res = parse(spark.Job("j0", [], 0),
                "E1.1: [[fist|s1]] -> [[verb|t1]] on [[pad|k1]]")
    assert len(res["sentences"]) == 1
    assert res["sentences"][0]["id"] == "e000001.1"
    with pytest.raises(spark.ParseError):
        parse(spark.Job("j0", [], 0), "E1.1: nothing marked")


# --- edits.py -------------------------------------------------------------


def make_ctx(seed):
    rng = random.Random(seed)
    return ed.Ctx(rng, CFG, ONTO, {}, ["Serum", "Pro-Q 3"])


def build_all(seed, n):
    ctx = make_ctx(seed)
    rng = ctx.rng
    out = []
    for i in range(1, n + 1):
        lang = ctx.wc(CFG["languages"])
        out.append(ed.build_edit(i, ctx, lang))
    return out


def test_edits_determinism():
    a = build_all(7, 60)
    b = build_all(7, 60)
    assert json.dumps(a, sort_keys=True) == json.dumps(b, sort_keys=True)


def test_edits_constraints():
    all_edits = build_all(3, 400)
    intents = {e["intent"] for e in all_edits}
    assert intents <= set(CFG["intents"])
    for e in all_edits:
        assert len(e["variants"]) == CFG["variants_per_edit"]
        srcs = [s["value"] for s in e["slots"].values()
                if s["kind"] == "source"]
        if e["intent"] == "map":
            assert len(srcs) == len(set(srcs))  # no source twice
        for r in e["routes"]:
            pvals = [e["slots"][t]["value"] for t in r["tgt"]
                     if e["slots"][t].get("target") == "param"]
            assert len(pvals) == len(set(pvals))
        # midi.note targets always get an x (note) slot
        if any(s.get("target") == "midi.note"
               for s in e["slots"].values()):
            assert any(s["kind"] == "note" for s in e["slots"].values())
        for r in e["routes"]:
            for t in r["tgt"]:
                if e["slots"][t].get("target") == "midi.note" \
                        and e["intent"] == "map":
                    assert any(e["slots"][m]["kind"] == "note"
                               for m in r["mods"])
            lo = [e["slots"][m]["value"] for m in r["mods"]
                  if e["slots"][m]["kind"] == "range_lo"]
            hi = [e["slots"][m]["value"] for m in r["mods"]
                  if e["slots"][m]["kind"] == "range_hi"]
            if lo and hi:
                assert lo[0] < hi[0]
            for m in lo + hi:
                assert 0.05 <= m <= 0.95 and abs(m * 20 - round(m * 20)) < 1e-9
    # replace payload points at the from/to slots
    for e in all_edits:
        if e["intent"] == "replace":
            assert e["slots"]["from1"]["kind"] == "target_from"
            assert e["slots"]["to1"]["kind"] == "target_to"
            assert (e["slots"]["from1"]["value"]
                    != e["slots"]["to1"]["value"])
    # distribution sanity: map dominates
    n_map = sum(1 for e in all_edits if e["intent"] == "map")
    assert n_map > 0.4 * len(all_edits)


# ---------- v2: new slots, negative check, keywords, roundtrip -------

def test_validate_new_slots():
    """rf/g/tm/dl/w slots must be marked exactly once like any other."""
    e = {"id": "e9", "lang": "en", "intent": "modify",
         "variants": [{"register": "x", "noise": "clean",
                       "frame": "imperative"}],
         "slots": {
             "rf1": {"kind": "ref", "value": "ref:last_route"},
             "dl1": {"kind": "delta", "value": 0.25},
             "g1": {"kind": "gate", "value": "hand.fist"},
             "tm1": {"kind": "time", "value": "2 bars"},
             "w1": {"kind": "rejected", "of": "t1",
                    "value": "fx.mix"},
         },
         "routes": [{"ref": "rf1", "src": [], "tgt": [],
                     "scope": [], "mods": ["dl1", "g1", "tm1"]}]}
    ok, _, _ = commands.validate_variant(
        e, 1, "make [[that one|rf1]] [[a bit more|dl1]] "
              "[[only while my fist|g1]] over [[2 bars|tm1]] "
              "no wait not [[the mix|w1]]")
    assert ok
    ok, reason, _ = commands.validate_variant(
        e, 1, "make [[that one|rf1]] [[a bit more|dl1]] "
              "[[only while my fist|g1]] over [[2 bars|tm1]]")
    assert not ok and "w1" in reason


def test_negative_check():
    commands.NEGATIVE.clear()
    commands.NEGATIVE.update({
        "source:hand.y": [("en", "raising my hand")],
        "param:fx.reverb.mix": [("en", "reverb")],
        "family:delay": [("en", "the delay")],
    })
    e = {"id": "e1", "lang": "en", "intent": "map",
         "variants": [{}],
         "slots": {"s1": {"kind": "source", "value": "mouth.open"},
                   "t1": {"kind": "target", "target": "param",
                          "value": "filter.cutoff"}},
         "routes": []}
    # 'the delay' outside marks -> extra:family:delay
    reason = commands.negative_check(
        e, "map my mouth to the cutoff and the delay",
        [{"start": 7, "end": 12, "slot": "s1", "text": "mouth"},
         {"start": 19, "end": 25, "slot": "t1", "text": "cutoff"}])
    assert reason == "extra:family:delay"
    # own concept phrase is fine
    reason = commands.negative_check(
        e, "[[open mouth|s1]] drives [[the filter|t1]] reverb",
        [{"start": 0, "end": 10, "slot": "s1", "text": "open mouth"},
         {"start": 18, "end": 28, "slot": "t1",
          "text": "the filter"}])
    # 'reverb' is a distinctive param phrase not in this edit
    assert reason == "extra:param:fx.reverb.mix"
    commands.NEGATIVE.clear()


def test_keyword_spans():
    import random
    rng = random.Random(1)
    phrases = {"source:hand.y": ["hand height"],
               "param:filter.cutoff": ["cutoff"],
               "side:right": ["right"]}
    e = {"id": "e1", "lang": "en", "intent": "map",
         "variants": [{"register": "x", "noise": "clean",
                       "frame": "imperative"}],
         "slots": {
             "s1": {"kind": "source", "value": "hand.y",
                    "side": "right"},
             "t1": {"kind": "target", "target": "param",
                    "value": "filter.cutoff"},
             "k1": {"kind": "track", "value": "pad"}},
         "routes": [{"src": ["s1"], "tgt": ["t1"],
                     "scope": ["k1"], "mods": []}]}
    import keywords
    row = keywords.build_sentence(e, phrases, rng)
    assert row and row["register"] == "keyword"
    for sp in row["spans"]:
        assert row["text"][sp["start"]:sp["end"]] == sp["text"]


def test_roundtrip_folding():
    import roundtrip
    # fist on the left -> openness + flip
    s, side, flip = roundtrip.fold_src("hand.fist:left")
    assert (s, side, flip) == ("hand.openness", "left", True)
    s, side, flip = roundtrip.fold_src("hand.y")
    assert (s, side, flip) == ("hand.y", None, False)
    # gold: fist + invert -> folded inv = False
    e = {"intent": "map",
         "slots": {"s1": {"kind": "source", "value": "hand.fist"},
                   "t1": {"kind": "target", "target": "param",
                          "value": "filter.cutoff"},
                   "i1": {"kind": "invert", "value": True}},
         "routes": [{"src": ["s1"], "tgt": ["t1"], "scope": [],
                     "mods": ["i1"]}]}
    # pred says openness, inv false -> folded gold inv also False
    pred = {"intent": "map", "routes": [
        {"src": ["hand.openness"], "tgt": ["filter.cutoff"],
         "inv": False}]}
    assert roundtrip.compare(e, pred) == {}
    # pred inv true -> mismatch on invert
    pred["routes"][0]["inv"] = True
    bad = roundtrip.compare(e, pred)
    assert "r1.invert" in bad


def test_copy_chain_plugin_then_template():
    label_of = {
        ("B", 5): ("copy", {"copied_from": "A"}),
        ("A", 5): ("copy", {"rep_index": 2}),
        ("A", 2): ("filter.cutoff", 1, 0, 1, "spark", "j0"),
    }
    assert list(label._copy_chain(label_of, "B", 5)) == [("A", 5), ("A", 2)]
    assert label._copy_resolved(label_of, {("A", 2): {}}, "B", 5)


def test_placeholder_matches_fl_rs():
    """fl_param_counts.placeholder (also export_paramsense.py's) against
    the cases of fl.rs's own `placeholders` test, read from the source."""
    src = (HERE.parents[1] / "plugin" / "synthexis" / "src" / "fl.rs").read_text()
    body = src[src.index("fn placeholders()"):]
    unescape = lambda s: re.sub(r"\\u\{([0-9a-fA-F]+)\}",  # noqa: E731
                                lambda m: chr(int(m.group(1), 16)), s)
    yes, no = [[unescape(x) for x in re.findall(r'"((?:[^"\\]|\\.)*)"', lst)]
               for lst in re.findall(r"for n in \[(.*?)\]\s*\{", body, re.S)[:2]]
    assert len(yes) >= 10 and len(no) >= 5 and "\x13" in yes
    assert [n for n in yes if not fl_param_counts.placeholder(n)] == []
    assert [n for n in no if fl_param_counts.placeholder(n)] == []
