"""Hand labels for Logic Pro 13's own plug-ins (param-sense classes from
tools/mapping/ontology.yaml), written 2026-11-13 by Claude from the control
names in manual_names.tsv (Apple's Logic Pro 13 User Guide): the scripted
rules of tools/mapping/ableton/stock_labels.py per plug-in family, Logic
rules in front of them, then per-plug-in overrides. No model calls.

  python3 tools/mapping/logic/stock_labels.py > tools/mapping/logic/stock_labels.tsv

Key: "logic:<plug-in name>" as the guide and Logic's plug-in menus name it.
Logic's own plug-ins are no Audio Units, so no AU in known.tsv can carry
their names, but generic names ("Compressor", "Chorus", "Sampler") would
still meet third-party plug-ins of that name; the prefix keeps them apart.
The names are the guide's control labels without the control word
("Threshold knob" -> "Threshold"). The names Logic shows for automation
and Controller Assignments can differ: status "UI label" until a probe in
Logic reads them.

Columns: plugin key, plug-in, kind (Effects or Instruments), guide page,
parameter name, class, status.
"""

import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "ableton"))
from stock_labels import A, MIXES, rule  # noqa: E402

FAMILY = {}
for fam, names in {
    "reverb": "ChromaVerb, Space Designer, SilverVerb, EnVerb, Quantec Room Simulator",
    "delay": "Tape Delay, Stereo Delay, Echo, Sample Delay, Delay Designer",
    "mod": "Chorus, Ensemble, Flanger, Microphaser, Phaser, Tremolo, Scanner Vibrato, Spreader, Modulation Delay,"
           " Rotor Cabinet",
    "dist": "Bitcrusher, Clip Distortion, Distortion, Distortion II, Overdrive, Phase Distortion, ChromaGlow,"
            " Amp Designer, Bass Amp Designer, Pedalboard, Exciter, SubBass",
    "dyn": "Compressor, Adaptive Limiter, Limiter, Multipressor, Expander, Enveloper, DeEsser 2",
    "eq": "Channel EQ, Linear Phase EQ, Match EQ, Single Band EQ, Vintage EQ Collection",
    "filter": "AutoFilter, Fuzz-Wah, EVOC 20 Filterbank",
    "pitch": "Pitch Correction, Vocal Transformer, Ringshifter",
    "res": "EVOC 20 TrackOscillator",
    "fx": "Phat FX, Step FX, Beat Breaker, Remix FX",
    "midi": "Arpeggiator, Modifier, Modulator, Note Repeater, Randomizer, Transposer, Velocity Processor",
    "synth": "Alchemy, Sculpture, Retro Synth, ES1, ES2, ES E, ES M, ES P, EFM1, EVOC 20 PolySynth,"
             " Vintage B3 Organ, Vintage Clav, Vintage Electric Piano, Vintage Mellotron, Studio Bass,"
             " Studio Horns, Studio Strings, Drum Synth, Ultrabeat",
    "sampler": "Sampler, Quick Sampler, Sample Alchemy",
}.items():
    for n in names.split(","):
        FAMILY[n.strip()] = fam
INSTR = {"synth", "sampler"}


def logic_rule(fam: str, n: str) -> str:
    """Logic's own words first, then the Live rules."""
    s = n.casefold().strip()
    mix = {"reverb": "rm", "delay": "dm", "mod": "mm"}.get(fam, "mix")
    if re.fullmatch(r"(dry/wet|mix|wet|output mix|wet level|effect mix|fx mix)", s):
        return mix
    if re.fullmatch(r"(dry|dry level|dry signal)", s):
        return "o"
    if re.search(r"^(polyphony|voices|used voices)$|via vel|velocity sensitivity|^velo |^velocity$|key ?scale", s):
        return "sys"
    if re.search(r"on/off$|^(bypass|freeze|solo|hq|q-couple|analyzer.*|invert|tempo sync|stereo link|latch)$", s):
        return "sw"
    if re.search(r"(delay )?time( l/r)?$|^note$", s) and fam == "delay":
        return "dt"
    if s in ("feedback",) and fam == "delay":
        return "df"
    if re.search(r"^(cutoff|lp cutoff|frequency)$", s) and (fam in INSTR or fam in ("filter", "fx")):
        return "cut"
    if re.search(r"^(res|reso|resonance|res\(onance\))$", s):
        return "res" if (fam in INSTR or fam in ("filter", "fx")) else "o"
    if re.search(r"^(size|decay|reverb time|length)$", s) and fam == "reverb":
        return "rd"
    if re.search(r"^(threshold|ratio|compression)$", s) and fam == "dyn":
        return "ca"
    if re.search(r"^(make ?up|output gain|gain)$", s) and fam == "dyn":
        return "u" if "make" in s else "g"
    if re.search(r"^(rate|speed|mod speed|lfo rate)$", s):
        return "lr" if fam in INSTR else "mr"
    if re.search(r"^(depth|intensity|mod depth|lfo intensity)$", s) and fam != "midi":
        return "la" if fam in INSTR else "md"
    if re.search(r"^(drive|distortion|dirt|crush|bit depth|downsampling|clip threshold)$", s):
        return "dr"
    if re.search(r"^(input|input gain|output|output level|level|volume|master gain|master|main out|vol)$", s):
        return "g"
    if re.search(r"^(attack|decay|sustain|release)( time)?$", s) and fam not in INSTR:
        return "o"
    return rule(fam, n)


# Per-plug-in overrides: "name=code; name=code" (codes: ableton A)
O = {
    "Channel EQ": "Master Gain=g; Processing=sel; Analyzer Decay=sys",
    "Compressor": "Circuit Type=sel; Side Chain=sys; Knee=o; Auto Gain=sel; Auto Release=sw; Limiter=sw;"
                  " Limiter Threshold=o; Distortion=sel; Detection=sel; Max=sel; Sum=sel; Peak/RMS=sel; Filter=sw;"
                  " Filter mode=sel; Frequency=sys; Q=sys; Gain=sys",
    "ChromaVerb": "Room Type=sel; Frequency=ef; Ratio=o; Q=eq; Gain=eg; Size=rd; Density=o; Predelay=o;"
                  " Predelay sync=sys; Decay sync=sys; Distance=o; Order=sel; Quality=sel; Mod Source=sel;"
                  " Smoothing=o; Early/Late Mix=o; Width=w; Mono Maker=o; Attack=o",
    "Tape Delay": "Delay Time=dt; Note=dt; Deviation=o; Smoothing=o; Clip Threshold=dr; Spread=w;"
                  " Tape Head Mode=sel; Low/High Cut=t; LFO Rate=mr; LFO Intensity=md; Flutter Rate/Intensity=md",
    "Stereo Delay": "Input=sel; Delay Time=dt; Note=dt; Deviation=o; Low/High Cut=t; Feedback Phase=sw;"
                    " Crossfeed Left to Right (Right to Left)=o; Crossfeed Phase=sw; Routing=sel",
    "Retro Synth": "Shape 1=os; Shape 2=os; Wavetable=os; Shape/Formant=os; Cents=of; Semitones=op;"
                   " FM (Amount)=fm; Filter Env(elope)=fe; Filter FM/Drive=dr; Sine Level=ol; Mix=ol;"
                   " Voice Detune=ud; Vibrato=la; Velocity=sys; Polyphony=sys; Key (Follow)=sys",
    "Space Designer": "Gain=eg; Q=eq",
    "Alchemy": "Frequency=o; Fine Tune=of; Speed=o",
    "ES P": "ADSR Int=fe; Overdrive=dr",
    "ES1": "Mix=ol; Out Level=g; Sub=ol",
    "EFM1": "FM Depth=fm; Main Level=g; Sub Osc Level=ol; Stereo Detune=ud",
    "Vintage Electric Piano": "Bass=eg; Treble=eg; Damper Noise=o",
    "Quick Sampler": "Env Depth=fe; Polyphony=sys; Root Key=sys; Key Tracking=sys; Vel=sys",
    "Amp Designer": "Gain=dr; Master=dr",
    "AutoFilter": "Fatness=o; Spread=w; Envelope=fe; LFO=la; Threshold=o; Dynamic=o; Pre Filter=dr;"
                  " Post Filter=dr; Sync Phase=o; Stereo Phase=w; Dry Signal=o; Main Out=g; State Variable=sel;"
                  " 4-Pole Lowpass=sel; Attack=o; Decay=o; Sustain=o; Release=o; Pulse Width=o",
}


def overrides():
    out = {}
    for plug, spec in O.items():
        for item in filter(None, (x.strip() for x in spec.split(";"))):
            name, code = item.rsplit("=", 1)
            assert code in A, (plug, item)
            out[(plug, name.strip())] = code
    return out


def main() -> None:
    ov, used, rows = overrides(), set(), []
    for line in (HERE / "manual_names.tsv").read_text(encoding="utf-8").splitlines()[1:]:
        kind, plug, page, name = line.split("\t")
        fam = FAMILY[plug]
        code = ov.get((plug, name))
        if code:
            used.add((plug, name))
        else:
            code = logic_rule(fam, name)
        c = A[code]
        if re.search(r"\bdry\b", name, re.I) and not re.search(r"wet", name, re.I) and c in MIXES:
            c = "other"
        rows.append(f"logic:{plug}\t{plug}\t{kind}\t{page}\t{name}\t{c}\tUI label")
    stale = set(ov) - used
    assert not stale, sorted(stale)
    print("plugin\tplugin_name\tkind\tpage\tname\tclass\tstatus")
    print("\n".join(rows))


if __name__ == "__main__":
    main()
