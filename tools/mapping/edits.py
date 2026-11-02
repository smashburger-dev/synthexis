# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml"]
# ///
"""Deterministic sampler of structured edits (no LLM).

  uv run tools/mapping/edits.py --run <name> --n <edits> --seed <s>
      [--glossary-run <name>] [--override edits-s4.yaml,edits-s5.yaml] [--id-prefix f]

Implements edits.yaml exactly (blocks in each --override file, in order,
replace or update the ones of the same name); writes
data/mapping/commands/<run>/edits.jsonl and prints the realised
distribution for comparison with edits.yaml.
"""

import argparse
import json
import random
import sys
import copy
from collections import Counter
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
EVENT_SOURCES = {
    "motion.throw", "motion.swipe", "motion.hold",
    "motion.grab", "motion.release", "motion.clap",
}
MOVE_SOURCES = {"hand.x", "hand.y", "hand.depth", "hand.roll", "hand.pitch"}
HAND_PRESETS = {"filter_hand", "chord_hand", "pattern_hand"}
FINGERS = ["thumb", "index", "middle", "ring", "pinky"]
FINGER_SHAPE = {"thumb": "thumbs_up", "index": "point", "pinky": "pinky_up"}
AMOUNT_WORD = {0.25: "subtle", 0.5: "medium", 0.75: "strong", 1.0: "full"}


class Ctx:
    def __init__(self, rng, cfg, onto, glossary, harvest_names):
        self.rng = rng
        self.cfg = cfg
        self.onto = onto
        self.glossary = glossary          # (lang, concept) -> [rec]
        self.harvest_names = harvest_names
        self.side_ids = {s["id"] for s in onto["sources"] if s.get("side")}
        self.named_ids = {s["id"] for s in onto["sources"] if s.get("named")}
        self.centred_ids = {s["id"] for s in onto["sources"]
                            if s.get("centred")}
        self.pair = {s["id"]: s["pair"] for s in onto["sources"]
                     if s.get("pair")}
        self.shape_ids = [s["id"] for s in onto["shapes"]]
        self.taxo_quantity = {t["id"]: t["quantity"] for t in onto["taxonomy"]}
        self.slots: dict = {}
        self.counters = Counter()

    def add(self, prefix: str, slot: dict) -> str:
        self.counters[prefix] += 1
        sid = f"{prefix}{self.counters[prefix]}"
        self.slots[sid] = slot
        return sid

    def wc(self, dist: dict):
        keys = list(dist)
        return self.rng.choices(
            keys, weights=[dist[k] for k in keys], k=1)[0]

    def draw_source(self, used: set) -> str:
        for _ in range(20):
            s = self.wc(self.cfg["sources"])
            if s not in used:
                used.add(s)
                return s
        used.add(s)
        return s

    def source_slot(self, sid_value: str) -> dict:
        side = object_ = shape = None
        if sid_value in self.side_ids:
            side = self.wc(self.cfg["sides"])
            if side == "none":
                side = None
        if sid_value in self.named_ids:
            object_ = self.wc(self.cfg["objects"])
        if sid_value == "hand.shape":
            shape = self.rng.choice(self.shape_ids)
            if self.cfg.get("shape_mask") and \
                    self.rng.random() < self.cfg["shape_mask"]:
                shape = self.mask_shape()
        return {"kind": "source", "value": sid_value, "side": side,
                "object": object_, "shape": shape, "say": None}

    def mask_shape(self) -> str:
        """Any finger combination that is not a named shape: 'm01010'."""
        named = {s["mask"] for s in self.onto["shapes"]}
        while True:
            m = format(self.rng.randrange(1, 32), "05b")
            if m not in named:
                return "m" + m

    def draw_track(self) -> str:
        lst = (self.cfg["tracks"]["roles"] if self.rng.random() < 0.7
               else self.cfg["tracks"]["custom"])
        return self.rng.choice(lst)

    def draw_plugin(self) -> str:
        if self.harvest_names and self.rng.random() < 0.5:
            if self.rng.random() < 0.3:
                return self.rng.choice(self.cfg["demo_plugins"])
            return self.rng.choice(self.harvest_names)
        return self.rng.choice(self.cfg["fl_plugins"])

    def draw_target_kind(self, route_srcs: list) -> str:
        values = [self.slots[s]["value"] for s in route_srcs]
        if "hand.shape" in values:
            return self.wc(self.cfg["shape_targets"])
        if any(v in EVENT_SOURCES for v in values):
            return self.wc(self.cfg["event_targets"])
        return self.wc(self.cfg["targets"])

    def draw_param(self, used: set) -> str:
        for _ in range(20):
            p = self.wc(self.cfg["params"])
            if p not in used:
                used.add(p)
                return p
        return p

    def add_target(self, kind: str, used_params: set,
                   mods: list) -> str:
        if kind == "param":
            tid = self.add("t", {
                "kind": "target", "target": "param",
                "value": self.draw_param(used_params), "say": None})
        elif kind == "macro":
            lo, hi = self.cfg["macro_signal"]
            tid = self.add("t", {
                "kind": "target", "target": "macro",
                "value": f"Signal {self.rng.randint(lo, hi)}",
                "say": None})
        elif kind == "toggle":
            tid = self.add("t", {
                "kind": "target", "target": "toggle",
                "value": self.wc(self.cfg["toggle_families"]),
                "mode": self.wc(self.cfg["toggle_modes"]),
                "say": None})
        elif kind == "midi.note":
            tid = self.add("t", {
                "kind": "target", "target": "midi.note",
                "value": None, "say": None})
            mods.append(self.add("x", {
                "kind": "note",
                "value": self.rng.choice(self.cfg["midi_notes"])}))
        elif kind == "midi.pattern":
            tid = self.add("t", {
                "kind": "target", "target": "midi.pattern",
                "value": self.rng.choice(self.cfg["midi_patterns"]),
                "say": None})
        elif kind == "midi.chord":
            tid = self.add("t", {
                "kind": "target", "target": "midi.chord",
                "value": None, "say": None})
            if self.cfg.get("chord_named") and \
                    self.rng.random() < self.cfg["chord_named"]:
                self.add_chord(mods)
        elif kind == "midi.prop":
            tid = self.add("t", {
                "kind": "target", "target": "midi.prop",
                "value": self.wc(self.cfg["midi_props"]), "say": None})
            if self.slots[tid]["value"] == "arp.style":
                mods.append(self.add("v", {
                    "kind": "value",
                    "value": self.rng.choice(self.cfg["arp_styles"])}))
        elif kind == "midi.cc":
            tid = self.add("t", {
                "kind": "target", "target": "midi.cc",
                "value": self.rng.choice(self.cfg["midi_cc"]),
                "say": None})
        else:
            raise ValueError(f"target kind {kind}")
        return tid

    def add_chord(self, mods: list) -> None:
        used = {self.slots[m]["value"] for m in self.slots
                if self.slots[m]["kind"] == "chord"}
        vals = [v for v in self.cfg["chord_values"] if v not in used] \
            or self.cfg["chord_values"]
        mods.append(self.add("ch", {"kind": "chord",
                                    "value": self.rng.choice(vals)}))

    def add_modifier(self, kind: str, mods: list) -> None:
        rng = self.rng
        if kind == "bipolar":
            mods.append(self.add("bp", {"kind": "bipolar", "value": True}))
        elif kind == "invert":
            mods.append(self.add("i", {"kind": "invert", "value": True}))
        elif kind == "range":
            shape = self.wc(self.cfg["range_shapes"])
            vals = [round(x * 0.05, 2) for x in range(1, 20)]
            if shape == "up_to":
                mods.append(self.add("hi", {
                    "kind": "range_hi", "value": rng.choice(vals)}))
            elif shape == "from":
                mods.append(self.add("lo", {
                    "kind": "range_lo", "value": rng.choice(vals)}))
            else:
                lo, hi = sorted(rng.sample(vals, 2))
                mods.append(self.add("lo", {
                    "kind": "range_lo", "value": lo}))
                mods.append(self.add("hi", {
                    "kind": "range_hi", "value": hi}))
        elif kind == "curve":
            mods.append(self.add("c", {
                "kind": "curve",
                "value": self.wc(self.cfg["curve_values"])}))
        elif kind == "amount":
            mods.append(self.add("a", {
                "kind": "amount",
                "value": self.wc(self.cfg["amount_values"])}))
        elif kind == "smooth":
            mods.append(self.add("m", {
                "kind": "smooth",
                "value": self.wc(self.cfg["smooth_values"])}))
        elif kind == "hold":
            mods.append(self.add("o", {
                "kind": "hold",
                "value": self.wc(self.cfg["hold_values"])}))
        elif kind == "quantize":
            mods.append(self.add("q", {
                "kind": "quantize",
                "value": self.wc(self.cfg["quantize_values"])}))
        elif kind == "gate":
            gs = self.wc(self.cfg["gate_sources"])
            slot = {"kind": "gate", "value": gs,
                    "shape": None, "side": None}
            if gs == "hand.shape":
                slot["shape"] = self.rng.choice(self.shape_ids)
            elif gs in self.side_ids and rng.random() < 0.5:
                side = self.wc(self.cfg["sides"])
                slot["side"] = None if side == "none" else side
            mods.append(self.add("g", slot))
        elif kind == "time":
            mods.append(self.add("tm", {
                "kind": "time",
                "value": self.wc(self.cfg["time_values"])}))

    def maybe_correction(self) -> None:
        if self.rng.random() >= self.cfg["correction"]["probability"]:
            return
        want = self.wc(self.cfg["correction"]["of"])
        kmap = {"source": "source", "target": "target",
                "track": "track"}
        cands = [sid for sid, sl in self.slots.items()
                 if sl["kind"] == kmap[want]]
        if not cands:
            return
        of = self.rng.choice(cands)
        cur = self.slots[of]["value"]
        for _ in range(10):
            if want == "source":
                val = self.wc(self.cfg["sources"])
            elif want == "target":
                val = self.wc(self.cfg["params"])                     if self.slots[of].get("target") == "param"                     else self.wc(self.cfg["toggle_families"])
            else:
                val = self.draw_track()
            if val != cur:
                break
        self.add("w", {"kind": "rejected", "of": of,
                       "value": val})

    # --- intent bodies -------------------------------------------------

    def gen_map(self) -> list:
        if self.cfg.get("finger_set") and \
                self.rng.random() < self.cfg["finger_set"]:
            return self.gen_finger_set()
        if self.cfg.get("xy") and self.rng.random() < self.cfg["xy"]:
            return self.gen_xy()
        routes = []
        used_sources: set = set()
        first_k = None
        for _ in range(self.wc(self.cfg["routes_per_edit"])):
            srcs, tgts, scope, mods = [], [], [], []
            for _ in range(self.wc(self.cfg["sources_per_route"])):
                sid = self.add("s", self.source_slot(
                    self.draw_source(used_sources)))
                srcs.append(sid)
            used_params: set = set()
            values = [self.slots[x]["value"] for x in srcs]
            gated = self.tilt_gate(srcs, mods) or self.pinch_hold(srcs, mods)
            for _ in range(self.wc(self.cfg["targets_per_route"])):
                kind = self.draw_target_kind(srcs)
                tgts.append(self.add_target(kind, used_params, mods))
            if self.cfg.get("trigger_params"):
                self.trigger_param(values, tgts, mods)
            time_boost = (
                any(v in EVENT_SOURCES for v in values)
                or any(self.slots[t]["target"] in
                       ("toggle", "midi.pattern") for t in tgts))
            sc = self.wc(self.cfg["scope"])
            track_sid = None
            if sc in ("track", "track_plugin"):
                if first_k is not None and self.rng.random() < 0.3:
                    scope.append(first_k)
                    track_sid = first_k
                else:
                    k = self.add("k", {
                        "kind": "track", "value": self.draw_track()})
                    scope.append(k)
                    track_sid = k
                    if first_k is None:
                        first_k = k
            if sc in ("plugin", "track_plugin"):
                scope.append(self.add("p", {
                    "kind": "plugin", "value": self.draw_plugin()}))
            if sc == "param_name":
                scope.append(self.add("n", {
                    "kind": "param_name",
                    "value": self.rng.choice(self.cfg["param_names"])}))
            for kind, prob in self.cfg["modifiers"].items():
                if kind == "bipolar" and not any(
                        v in self.centred_ids for v in values):
                    continue
                if kind == "gate" and gated:
                    continue
                p_eff = prob if (kind != "time" or time_boost)                     else prob / 3
                if self.rng.random() < p_eff:
                    self.add_modifier(kind, mods)
            if track_sid and self.rng.random() < \
                    self.cfg["refs"]["track"]:
                self.slots[track_sid]["value"] = "ref:last_track"
            routes.append({"src": srcs, "tgt": tgts,
                           "scope": scope, "mods": mods})
        if routes and self.rng.random() < self.cfg["refs"]["map"]:
            self.add("rf", {"kind": "ref",
                            "value": "ref:last_route"})
            k = self.add("k", {"kind": "track",
                               "value": self.draw_track()})
            routes[-1] = {"ref": "rf1", "src": [], "tgt": [],
                          "scope": [k], "mods": []}
        return routes

    def trigger_param(self, values: list, tgts: list, mods: list) -> None:
        """A finger state or a single finger fires a parameter: jump to a
        value (or from one value to another), step to the next setting, or
        start a slow ramp that the next trigger reverses."""
        if not any(v == "hand.shape" or v.startswith("hand.finger.")
                   and not v.endswith("tilt") for v in values):
            return
        for t in tgts:
            sl = self.slots[t]
            if sl.get("target") != "param":
                continue
            how = self.wc(self.cfg["trigger_params"])
            if how in ("step", "ramp"):
                sl["mode"] = how
            elif how in ("value", "from_to"):
                for _ in range(1 if how == "value" else 2):
                    mods.append(self.add("v", {
                        "kind": "value",
                        "value": self.rng.choice(self.cfg["param_values"])}))

    def gen_xy(self) -> list:
        """One hand moving around drives a two-axis pad (Portal's macros,
        FabFilter XY controllers); sometimes only while pinching."""
        side = self.wc(self.cfg["sides"])
        side = None if side == "none" else side
        srcs = [self.add("s", {"kind": "source", "value": v, "side": side,
                               "object": None, "shape": None, "say": None})
                for v in ("hand.x", "hand.y")]
        t = self.add("t", {"kind": "target", "target": "xy", "value": None,
                           "say": None})
        scope, mods = [], []
        if self.rng.random() < 0.5:
            scope.append(self.add("p", {"kind": "plugin",
                                        "value": self.draw_plugin()}))
        if self.rng.random() < 0.4:
            mods.append(self.add("g", {"kind": "gate", "value": "hand.pinch",
                                       "shape": None, "side": side}))
        return [{"src": srcs, "tgt": [t], "scope": scope, "mods": mods}]

    def pinch_hold(self, srcs: list, mods: list) -> bool:
        """A hand movement that only counts while pinching ("pinch and
        drag", "pinch and turn"): the pinch is the hold, not a source."""
        if not self.cfg.get("pinch_hold"):
            return False
        moving = [x for x in srcs if self.slots[x]["value"] in MOVE_SOURCES]
        if not moving or self.rng.random() >= self.cfg["pinch_hold"]:
            return False
        mods.append(self.add("g", {"kind": "gate", "value": "hand.pinch",
                                   "shape": None,
                                   "side": self.slots[moving[0]]["side"]}))
        return True

    def tilt_gate(self, srcs: list, mods: list) -> bool:
        """Finger tilt only while that finger alone is shown (thumbs up)."""
        if not self.cfg.get("tilt_gate"):
            return False
        for x in srcs:
            s = self.slots[x]
            parts = s["value"].split(".")
            if len(parts) == 4 and parts[3] == "tilt" \
                    and parts[2] in FINGER_SHAPE \
                    and self.rng.random() < self.cfg["tilt_gate"]:
                mods.append(self.add("g", {
                    "kind": "gate", "value": "hand.shape",
                    "shape": FINGER_SHAPE[parts[2]], "side": s["side"]}))
                return True
        return False

    def gen_finger_set(self) -> list:
        """One hand, each finger its own route ('left hand: thumb Am,
        index F'); the hand side is one h slot for all routes."""
        cfg, rng = self.cfg, self.rng
        if rng.random() < 0.8:
            self.add("h", {"kind": "side",
                           "value": rng.choice(["left", "right"])})
        n = self.wc(cfg["finger_set_sizes"])
        fingers = sorted(rng.sample(FINGERS, n), key=FINGERS.index)
        kind = self.wc(cfg["finger_set_targets"])
        k = None
        if rng.random() < 0.6:
            k = self.add("k", {"kind": "track", "value": self.draw_track()})
        used_params: set = set()
        routes = []
        for f in fingers:
            s = self.add("s", {"kind": "source", "value": f"hand.finger.{f}",
                               "side": None, "object": None, "shape": None,
                               "say": None})
            mods: list = []
            t = self.add_target(kind, used_params, mods)
            if kind == "midi.chord" and not any(
                    self.slots[m]["kind"] == "chord" for m in mods):
                self.add_chord(mods)
            routes.append({"src": [s], "tgt": [t],
                           "scope": [k] if k else [], "mods": mods})
        return routes

    def _ref(self) -> tuple[list, list]:
        """One route ref: src+tgt 50 %, src 30 %, tgt 20 %."""
        r = self.rng.random()
        srcs, tgts = [], []
        if r < 0.8:
            srcs.append(self.add("s", self.source_slot(
                self.draw_source(set()))))
        if r < 0.5 or r >= 0.8:
            used: set = set()
            tgts.append(self.add_target(
                self.wc(self.cfg["targets"]), used, []))
        return srcs, tgts

    def gen_modify(self) -> list:
        mods: list = []
        n = self.wc(self.cfg["modify_mods_per_edit"])
        for kind in self._weighted_mod_kinds(n):
            if self.rng.random() < self.cfg["modify_delta_share"]:
                mods.append(self.add("dl", {
                    "kind": "delta",
                    "value": self.wc(self.cfg["delta_values"])}))
            else:
                self.add_modifier(kind, mods)
        if self.rng.random() < self.cfg["refs"]["modify"]:
            self.add("rf", {"kind": "ref",
                            "value": "ref:last_route"})
            return [{"ref": "rf1", "src": [], "tgt": [],
                     "scope": [], "mods": mods}]
        srcs, tgts = self._ref()
        return [{"src": srcs, "tgt": tgts, "scope": [], "mods": mods}]

    def _weighted_mod_kinds(self, n: int) -> list:
        pool = dict(self.cfg["modifiers"])
        out = []
        for _ in range(min(n, len(pool))):
            k = self.wc(pool)
            out.append(k)
            del pool[k]
        return out

    def gen_unmap(self) -> list:
        if self.rng.random() < self.cfg["refs"]["unmap"]:
            self.add("rf", {"kind": "ref",
                            "value": "ref:last_route"})
            return [{"ref": "rf1", "src": [], "tgt": [],
                     "scope": [], "mods": []}]
        n = self.wc(self.cfg["modify_mods_per_edit"])
        routes = []
        for _ in range(n):
            srcs, tgts = self._ref()
            routes.append({"src": srcs, "tgt": tgts,
                           "scope": [], "mods": []})
        return routes

    def gen_replace(self) -> None:
        if self.rng.random() < 0.6:
            self.add("s", self.source_slot(self.draw_source(set())))
        pool = {k: v for k, v in self.cfg["params"].items()
                if k not in ("other", "macro")}
        f = self.wc(pool)
        t = self.wc({k: v for k, v in pool.items() if k != f})
        self.add("from", {"kind": "target_from", "target": "param",
                          "value": f, "say": None})
        self.add("to", {"kind": "target_to", "target": "param",
                        "value": t, "say": None})

    def gen_scoped_slot(self, scope: str) -> None:
        if scope == "source":
            self.add("s", self.source_slot(self.draw_source(set())))
        elif scope == "target":
            self.add("t", {"kind": "target", "target": "param",
                           "value": self.draw_param(set()), "say": None})
        elif scope == "track":
            self.add("k", {"kind": "track", "value": self.draw_track()})

    def gen_other(self, intent: str) -> dict:
        cfg = self.cfg
        if intent == "clear":
            scope = self.wc(cfg["clear_scope"])
            if scope != "all":
                self.gen_scoped_slot(scope)
            return {"scope": scope}
        if intent == "role" and cfg.get("role_scope") and \
                self.rng.random() < cfg["role_scope"]:
            if self.rng.random() < 0.5:
                self.add("k", {"kind": "track", "value": self.draw_track()})
            else:
                self.add("p", {"kind": "plugin", "value": self.draw_plugin()})
            self.add("ro", {"kind": "role",
                            "value": self.wc(cfg["scoped_roles"])})
            return {}
        if intent == "role":
            self.add("h", {"kind": "side",
                           "value": self.rng.choice(["left", "right"])})
            self.add("ro", {"kind": "role",
                            "value": self.wc(cfg["roles"])})
            return {}
        if intent == "preset":
            v = self.wc(cfg["presets"])
            self.add("pr", {"kind": "preset", "value": v})
            if v in HAND_PRESETS and self.rng.random() < 0.5:
                self.add("h", {"kind": "side",
                               "value": self.rng.choice(
                                   ["left", "right"])})
            return {}
        if intent == "scale":
            self.add("key", {"kind": "key",
                             "value": self.rng.choice(cfg["keys"])})
            self.add("sc", {"kind": "scale",
                            "value": self.wc(cfg["scales"])})
            return {}
        if intent == "undo":
            self.add("u", {"kind": "undo",
                           "value": self.wc(cfg["undo"])})
            return {}
        if intent == "query":
            scope = self.wc(cfg["query_scope"])
            if scope != "all":
                self.gen_scoped_slot(scope)
            return {"scope": scope}
        return {}

    # --- say ------------------------------------------------------------

    def say_phrase(self, lang: str, concept: str, pol: int):
        cands = [r for r in self.glossary.get((lang, concept), [])
                 if r["pol"] == pol]
        if not cands:
            return None
        typos = [r for r in cands if r.get("typo")]
        if typos and self.rng.random() < 0.10:
            pool = typos
        else:
            pool = [r for r in cands if not r.get("typo")] or cands
        if self.cfg.get("say_rank_weighting"):
            return self.rng.choices(
                pool, weights=[1 / (i + 2) for i in range(len(pool))],
                k=1)[0]["p"]
        return self.rng.choice(pool)["p"]

    def apply_say(self, lang: str, routes: list) -> None:
        if not self.glossary:
            return
        p_say = self.cfg["say_probability"]
        # which routes have invert, for param-target polarity
        inv_routes = {
            i for i, r in enumerate(routes)
            if any(self.slots[m]["kind"] == "invert" for m in r["mods"])}
        tgt_route = {}
        for i, r in enumerate(routes):
            for tid in r["tgt"]:
                tgt_route[tid] = i
        pair_say = self.cfg.get("pair_say", 0)
        for sid, s in self.slots.items():
            if pair_say and s["kind"] == "source" \
                    and s["value"] in self.pair \
                    and self.rng.random() < pair_say:
                phr = self.say_phrase(
                    lang, f"source:{self.pair[s['value']]}", -1)
                if phr:
                    s["say"] = phr
                    continue
            if self.rng.random() >= p_say:
                continue
            kind = s["kind"]
            concept = pol = None
            if kind == "source":
                concept, pol = f"source:{s['value']}", 1
            elif kind == "target" and s.get("target") == "param":
                pol = 1
                if tgt_route.get(sid) in inv_routes \
                        and self.rng.random() < 0.5:
                    pol = -1
                concept = f"param:{s['value']}"
            elif kind == "track" and s["value"] in self.onto["tracks"]:
                concept, pol = f"track:{s['value']}", 1
            elif kind == "amount":
                concept, pol = f"mod:amount.{s['value']}", 1
            elif kind == "curve":
                concept, pol = f"mod:curve.{s['value']}", 1
            elif kind == "smooth":
                concept, pol = f"mod:smooth.{s['value']}", 1
            elif kind == "hold":
                concept, pol = f"mod:hold.{s['value']}", 1
            if concept:
                s["say"] = self.say_phrase(lang, concept, pol)


def load_glossary(run_name: str | None) -> dict:
    if not run_name:
        return {}
    p = (ROOT / "data" / "mapping" / "glossary" / run_name
         / "glossary.jsonl")
    if not p.exists():
        return {}
    out: dict[tuple, list] = {}
    for line in p.read_text().splitlines():
        r = json.loads(line)
        out.setdefault((r["lang"], r["c"]), []).append(r)
    return out


def load_harvest_names() -> list:
    p = ROOT / "data" / "mapping" / "harvest" / "au" / "plugins.jsonl"
    if not p.exists():
        return []
    names = []
    seen = set()
    for line in p.read_text().splitlines():
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if r.get("status") != "ok":
            continue
        n = r.get("name")
        if n and n not in seen:
            seen.add(n)
            names.append(n)
    return names


def merge(base: dict, over: dict) -> dict:
    """Override blocks; nested dicts update key by key."""
    out = dict(base)
    for k, v in over.items():
        if isinstance(v, dict) and isinstance(out.get(k), dict) \
                and k not in ("routes_per_edit", "intents", "languages"):
            out[k] = merge(out[k], v)
        else:
            out[k] = v
    return out


def build_edit(n: int, ctx: Ctx, lang: str, prefix: str = "e") -> dict:
    intent = ctx.wc(ctx.cfg["intents"])
    ctx.slots, ctx.counters = {}, Counter()
    routes, payload = [], {}
    if intent == "map":
        routes = ctx.gen_map()
    elif intent == "modify":
        routes = ctx.gen_modify()
    elif intent == "unmap":
        routes = ctx.gen_unmap()
    elif intent == "replace":
        ctx.gen_replace()
        payload = {"from": "from1", "to": "to1"}
    else:
        payload = ctx.gen_other(intent)
    fbi = ctx.cfg.get("frames_by_intent") or {}
    allowed = set(fbi.get(intent, fbi.get("default", [])))
    if allowed:
        fdist = {k: v for k, v in
                 ctx.cfg["styles"]["frame"].items() if k in allowed}
    else:
        fdist = ctx.cfg["styles"]["frame"]
    variants = [
        {"register": ctx.wc(ctx.cfg["styles"]["register"]),
         "noise": ctx.wc(ctx.cfg["styles"]["noise"]),
         "frame": ctx.wc(fdist)}
        for _ in range(ctx.cfg["variants_per_edit"])]
    ctx.maybe_correction()
    ctx.apply_say(lang, routes)
    return {
        "id": f"{prefix}{n:06d}", "lang": lang, "variants": variants,
        "intent": intent, "slots": ctx.slots, "routes": routes,
        "payload": payload}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--glossary-run", default=None)
    ap.add_argument("--override", default=None)
    ap.add_argument("--id-prefix", default="e")
    args = ap.parse_args()

    cfg = yaml.safe_load((HERE / "edits.yaml").read_text())
    for name in (args.override or "").split(","):
        if name:
            cfg = merge(cfg, yaml.safe_load((HERE / name).read_text()))
    onto = yaml.safe_load((HERE / "ontology.yaml").read_text())
    rng = random.Random(args.seed)
    glossary = load_glossary(args.glossary_run)
    ctx = Ctx(rng, cfg, onto, glossary, load_harvest_names())

    out_dir = ROOT / "data" / "mapping" / "commands" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)
    stats = {
        "intents": Counter(), "langs": Counter(),
        "sources": Counter(), "targets": Counter(),
        "mod_routes": Counter(), "n_routes": 0,
    }
    with (out_dir / "edits.jsonl").open("w") as f:
        for n in range(1, args.n + 1):
            lang = ctx.wc(cfg["languages"])
            e = build_edit(n, ctx, lang, args.id_prefix)
            f.write(json.dumps(e, ensure_ascii=False) + "\n")
            if e["intent"] == "map" and \
                    rng.random() < cfg["minimal_pairs"]["fraction"]:
                nb = cfg["minimal_pairs"]["neighbours"]
                twin = copy.deepcopy(e)
                cands = []
                for sid, sl in twin["slots"].items():
                    if sl["kind"] == "source" \
                            and sl["value"] in nb:
                        cands.append(sid)
                    elif sl["kind"] == "target" \
                            and sl.get("target") == "param" \
                            and sl["value"] in nb:
                        cands.append(sid)
                if cands:
                    sid = rng.choice(cands)
                    sl = twin["slots"][sid]
                    sl["value"] = rng.choice(nb[sl["value"]])
                    if glossary and \
                            rng.random() < cfg["say_probability"]:
                        c = (f"source:{sl['value']}"
                             if sl["kind"] == "source"
                             else f"param:{sl['value']}")
                        pool = [r for r in
                                glossary.get((lang, c), [])
                                if r.get("pol", 1) == 1]
                        sl["say"] = (ctx.say_phrase(lang, c, 1)
                                   if pool else None)
                    else:
                        sl["say"] = None
                    twin["id"] = e["id"] + "b"
                    f.write(json.dumps(twin, ensure_ascii=False)
                            + "\n")
            stats["intents"][e["intent"]] += 1
            stats["langs"][lang] += 1
            for r in e["routes"]:
                stats["n_routes"] += 1
                for m in r["mods"]:
                    stats["mod_routes"][
                        e["slots"][m]["kind"]] += 1
            for s in e["slots"].values():
                if s["kind"] == "source":
                    stats["sources"][s["value"]] += 1
                elif s["kind"] in ("target", "target_from",
                                   "target_to"):
                    stats["targets"][
                        f"{s.get('target')}:{s.get('value')}"] += 1
    nr = max(stats["n_routes"], 1)
    print(f"wrote {args.n} edits -> {out_dir / 'edits.jsonl'}")
    print("intents:", dict(stats["intents"].most_common()))
    print("langs:", dict(stats["langs"].most_common()))
    print("top sources:", dict(stats["sources"].most_common(12)))
    print("top targets:", dict(stats["targets"].most_common(12)))
    print("modifier rate per route:",
          {k: round(v / nr, 3) for k, v in
           stats["mod_routes"].most_common()})


if __name__ == "__main__":
    main()
