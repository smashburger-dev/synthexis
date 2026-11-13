"""Hand labels for the FL Studio stock effects with up to 60 parameters
(param-sense classes from tools/mapping/ontology.yaml), written 2026-11-13
by Claude from the FL dump (probes/fl-stock-plugins/RESULT.md): the paid
label run could not run that day (Baseten had deactivated
muse-spark-open-1026). One class code per parameter, in index order.

  python3 tools/mapping/fl/stock_labels.py <merged dump.json> > tools/mapping/fl/stock_labels.tsv

export_paramsense.py reads the TSV into known.tsv. Bigger stock plugins
(synths, Love Philter, Delay Bank, Distructor, ...) keep param-sense.
"""

import json, re, sys

MIXES = {"fx.reverb.mix", "fx.delay.mix", "fx.mod.mix", "fx.mix"}

A = {"g": "amp.gain", "pan": "amp.pan", "w": "stereo.width", "cut": "filter.cutoff", "res": "filter.resonance",
     "lr": "lfo.rate", "la": "lfo.amount", "ma": "mod.amount", "mac": "macro", "rm": "fx.reverb.mix",
     "rd": "fx.reverb.decay", "dm": "fx.delay.mix", "dt": "fx.delay.time", "df": "fx.delay.feedback",
     "mm": "fx.mod.mix", "mr": "fx.mod.rate", "md": "fx.mod.depth", "dr": "fx.drive", "t": "fx.tone",
     "mix": "fx.mix", "ca": "fx.comp.amount", "gate": "fx.gate", "eg": "fx.eq.gain", "ef": "fx.eq.freq",
     "eq": "fx.eq.q", "p": "fx.pitch", "sw": "ctl.switch", "sel": "ctl.select", "sys": "system",
     "o": "other", "u": "unsafe", "sp": "sample.position"}

L = {
 "Effector": "sel sw mix mac mac mr o ma ma sel g u",
 "Emphasis": "sw sw sel sys sel sel ca o ca g",
 "Emphasizer": "ca ca sel sw sw g",
 "Frequency Shifter": "mix sel p o o o md w sel o",
 "Frequency Splitter": "ef ef sel sel sel sel sys sys eg eg eg g g g",
 "Fruity 7 Band Equalizer": "eg eg eg eg eg eg eg",
 "Fruity Balance": "pan g",
 "Fruity Bass Boost": "ef eg",
 "Fruity Blood Overdrive": "t t dr sw t g",
 "Fruity Center": "sw",
 "Fruity Chorus": "o md w mr mr mr sel sel sel sel t sw",
 "Fruity Compressor": "ca ca u o o sel",
 "Fruity Convolver": "rm rm w w",
 "Fruity delay 2": "pan g dm df dt o sel t",
 "Fruity Delay 3": "dm sel sys sw dt o o w mr md md o o sel df sel t t dr dr dr dr dr dm t dm",
 "Fruity Delay": "dm df t sys dt sel",
 "Fruity Fast Dist": "dr dr sel mix g",
 "Fruity Fast LP": "cut res",
 "Fruity Filter": "cut res sel sel sel sw",
 "Fruity Flanger": "o md mr o t sel md sw sw mm mm o",
 "Fruity Flangus": "sel md mr o w o mm mm",
 "Fruity formula controller": "mac mac mac",
 "Fruity Free Filter": "sel cut res eg",
 "Fruity Limiter": "u dr ca o o o o o ca ca o o o o o gate gate o",
 "Fruity Multiband Compressor": "u sw sel sw ef g ca ca o o o sw ef ef g ca ca o o o sw ef g ca ca o o o",
 "Fruity Mute 2": "sw sel",
 "Fruity PanOMatic": "pan g sel sel la lr",
 "Fruity parametric EQ 2": " ".join(["eg"] * 7 + ["ef"] * 7 + ["eq"] * 7 + ["sel"] * 14 + ["u"]),
 "Fruity parametric EQ": " ".join(["eg"] * 7 + ["ef"] * 7 + ["eq"] * 7 + ["sel"] * 7 + ["g"]),
 "Fruity peak controller": "o ma o o o la o lr sel o",
 "Fruity Phase Inverter": "sel",
 "Fruity Phaser": "mr md md o w sel md mm g",
 "Fruity Reeverb 2": "t t o rd o rd t o o w rm o rm mr md",
 "Fruity Reeverb": "t t o rd o t rd t rm rm",
 "Fruity Scratcher": "sp sw o o pan g sw o sel o o o o o",
 "Fruity send": "pan g mix",
 "Fruity soft clipper": "dr g",
 "Fruity Squeeze": "g mix dr sw sel res cut sel sel o o dr",
 "Fruity Stereo Enhancer": "pan g w w sel sel",
 "Fruity Stereo Shaper": "w g g w w w",
 "Fruity Vocoder": "t t o sw p o o o sw g g mix",
 "Fruity WaveShaper": "dr mix g",
 "Fruity X-Y controller": "mac o o o mac o o o o o",
 "Fruity X-Y-Z Controller": "mac mac mac o o o o o o o o",
 "Gross Beat": "sel sel mix gate o o o sw o",
 "Hyper Chorus": "o md mr md md mr sel t t mm w",
 "Low Lifter": "sw mix eg o sys sw dr o o eg ef t sw sw sw",
 "LuxeVerb": ("rm t t sw rd o o t o rd mr md o sw sys sw p o o o t t o ma o o o o sel sw o o "
              "rm rm ef eg eq w sw sel o p p o o o o o o"),
 "Maximus": "ef sel ef sel sys mix t " + " ".join(["sw ca g w dr o o o o o o o sel"] * 3)
            + " sw ca u w dr o o o o o o o sel",
 "Pitch Shifter": "p p sys mix o o o o dt df sel o sel",
 "Spreader": "w w o sw sw",
 "Transient Processor": "o dr o g sel sel o mix sw",
 "Transporter": "mix mix mix o p p w o o o o sw sw sel sw sw sw sw sel sel sel o",
 "Vintage Chorus": "sel sw mr sel w o o mm sw sw md t g o sw sys o",
 "Vintage Phaser": ("md md md sw o sys mr sys sel o o w sw sw dm w dt sys df t t mm sw o sw eg ef t t "
                    "mr mr md o sel dr sw"),
 "Vocodex": "mix dr g t g t o t o o o o o sw o o o p o w",
}

dump = json.load(open(sys.argv[1]))
rows = []
for pl in dump["plugins"]:
    if pl["name"] not in L:
        continue
    cls = [A[c] for c in L.pop(pl["name"]).split()]
    ps = pl["params"]
    assert len(cls) == len(ps), (pl["name"], len(cls), len(ps))
    # a pure dry level keeps no mix class: on a class tie the resolver binds
    # the first parameter, and Dry comes before Wet in most of these
    # ("open the reverb" would raise Dry level on Fruity Reeverb 2)
    dry = lambda n: re.search(r"\bdry\b", n, re.I) and not re.search(r"wet", n, re.I)
    cls = ["other" if dry(q["name"]) and c in MIXES else c for q, c in zip(ps, cls)]
    rows += [f"{pl['name']}\t{q['index']}\t{q['name']}\t{c}" for q, c in zip(ps, cls)]
assert not L, L
print("plugin\tindex\tname\tclass")
print("\n".join(rows))
