"""Hand labels for Ableton Live's stock devices (param-sense classes from
tools/mapping/ontology.yaml), written 2026-11-13 by Claude from the LOM
parameter names in lom_names.tsv (see README.md for the sources): scripted
rules per device family, then per-device overrides where a rule is wrong.
No model calls.

  python3 tools/mapping/ableton/stock_labels.py > tools/mapping/ableton/stock_labels.tsv

Key: a remote script sees a stock device as `Device.class_name` ("Reverb",
"Eq8", "InstrumentVector") and its parameters by
`DeviceParameter.original_name`, which the user cannot rename. `Device.name`
is no key: loading a preset renames the device to the preset ("Large Hall").
The plugin column is "ableton:<class_name>", so no Live device can share a
known.tsv key with an AU/VST3 plugin ("Reverb", "Frequency Shifter", ...);
the future Live bridge asks sense.rs for "ableton:" + class_name.

Columns: plugin key; class_name; display name (class_display_name as the
browser shows it); display_in_manual (yes: a section of the Live 12.4
manual has that title, or the manual names it, as for Erosion Legacy; no:
from memory, UNVERIFIED); parameter name; class; status (12.4 when the 12.4
scripts list the name or the 12.4 manual uses it for that device, else the
newest Live whose scripts list it, 12.1 or 12.0; "UNVERIFIED in 12.4" for
a device 12.4 changed that neither 12.4 source confirms); manual (yes/no
from manual_12_4.tsv, "-" for a device without a manual section); sources
of the name (lom_names.tsv).
"""

import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

A = {"g": "amp.gain", "pan": "amp.pan", "w": "stereo.width", "cut": "filter.cutoff", "res": "filter.resonance",
     "fe": "filter.env_amount", "op": "osc.pitch", "of": "osc.fine", "os": "osc.shape", "ol": "osc.level",
     "fm": "osc.fm", "ud": "unison.detune", "gl": "voice.glide", "sp": "sample.position",
     "ea": "env.attack", "ed": "env.decay", "es": "env.sustain", "er": "env.release",
     "lr": "lfo.rate", "la": "lfo.amount", "ma": "mod.amount", "mac": "macro",
     "rm": "fx.reverb.mix", "rd": "fx.reverb.decay", "dm": "fx.delay.mix", "dt": "fx.delay.time",
     "df": "fx.delay.feedback", "mm": "fx.mod.mix", "mr": "fx.mod.rate", "md": "fx.mod.depth",
     "dr": "fx.drive", "t": "fx.tone", "mix": "fx.mix", "ca": "fx.comp.amount", "gate": "fx.gate",
     "eg": "fx.eq.gain", "ef": "fx.eq.freq", "eq": "fx.eq.q", "p": "fx.pitch",
     "sw": "ctl.switch", "sel": "ctl.select", "sys": "system", "o": "other", "u": "unsafe"}

MIXES = {"fx.reverb.mix", "fx.delay.mix", "fx.mod.mix", "fx.mix"}

# class_name: (display name, family). Families pick the mix, feedback,
# rate and filter classes of the rules below.
DEVICES = {
    # audio effects
    "Amp": ("Amp", "dist"), "AutoFilter": ("Auto Filter (legacy)", "filter"),
    "AutoFilter2": ("Auto Filter", "filter"), "AutoPan": ("Auto Pan (legacy)", "mod"),
    "AutoPan2": ("Auto Pan-Tremolo", "mod"), "AutoShift": ("Auto Shift", "pitch"),
    "BeatRepeat": ("Beat Repeat", "fx"), "Cabinet": ("Cabinet", "dist"), "ChannelEq": ("Channel EQ", "eq"),
    "Chorus": ("Chorus (legacy)", "mod"), "Chorus2": ("Chorus-Ensemble", "mod"),
    "Compressor2": ("Compressor", "dyn"), "Corpus": ("Corpus", "res"), "Delay": ("Delay", "delay"),
    "DrumBuss": ("Drum Buss", "dist"), "Echo": ("Echo", "delay"), "Eq8": ("EQ Eight", "eq"),
    "Erosion": ("Erosion Legacy", "dist"), "Erosion2": ("Erosion", "dist"),
    "FilterDelay": ("Filter Delay", "delay"), "FilterEQ3": ("EQ Three", "eq"),
    "Flanger": ("Flanger (legacy)", "mod"), "FrequencyShifter": ("Frequency Shifter (legacy)", "pitch"),
    "Gate": ("Gate", "dyn"), "GlueCompressor": ("Glue Compressor", "dyn"),
    "GrainDelay": ("Grain Delay", "delay"), "Hybrid": ("Hybrid Reverb", "reverb"),
    "Limiter": ("Limiter", "dyn"), "Looper": ("Looper", "fx"), "MultibandDynamics": ("Multiband Dynamics", "dyn"),
    "Overdrive": ("Overdrive", "dist"), "Pedal": ("Pedal", "dist"), "Phaser": ("Phaser (legacy)", "mod"),
    "PhaserNew": ("Phaser-Flanger", "mod"), "Redux": ("Redux (legacy)", "dist"), "Redux2": ("Redux", "dist"),
    "Resonator": ("Resonators", "res"), "Reverb": ("Reverb", "reverb"), "Roar": ("Roar", "dist"),
    "Saturator": ("Saturator", "dist"), "Shifter": ("Shifter", "pitch"), "Spectral": ("Spectral Time", "delay"),
    "StereoGain": ("Utility", "util"), "Transmute": ("Spectral Resonator", "res"), "Tube": ("Dynamic Tube", "dist"),
    "Vinyl": ("Vinyl Distortion", "dist"), "Vocoder": ("Vocoder", "res"),
    # instruments
    "Collision": ("Collision", "synth"), "Drift": ("Drift", "synth"), "DrumCell": ("Drum Sampler", "sampler"),
    "InstrumentImpulse": ("Impulse", "sampler"), "InstrumentMeld": ("Meld", "synth"),
    "InstrumentVector": ("Wavetable", "synth"), "LoungeLizard": ("Electric", "synth"),
    "MultiSampler": ("Sampler", "sampler"), "Operator": ("Operator", "synth"),
    "OriginalSimpler": ("Simpler", "sampler"), "StringStudio": ("Tension", "synth"),
    "UltraAnalog": ("Analog", "synth"),
    # racks
    "AudioEffectGroupDevice": ("Audio Effect Rack", "rack"), "InstrumentGroupDevice": ("Instrument Rack", "rack"),
    "DrumGroupDevice": ("Drum Rack", "rack"), "MidiEffectGroupDevice": ("MIDI Effect Rack", "rack"),
    # MIDI effects
    "MidiArpeggiator": ("Arpeggiator", "midi"), "MidiCcControl": ("CC Control", "midi"),
    "MidiChord": ("Chord", "midi"), "MidiNoteLength": ("Note Length", "midi"), "MidiPitcher": ("Pitch", "midi"),
    "MidiRandom": ("Random", "midi"), "MidiScale": ("Scale", "midi"), "MidiVelocity": ("Velocity", "midi"),
}
# Not labelled: ProxyAudioEffectDevice (a wrapper class, not a device in the
# browser; its three names are Dry/Wet, Input Gain, Output Gain).

INSTR = {"synth", "sampler"}

# Devices whose parameters changed after the 12.0/12.1 scripts: Live 12.4
# updated Delay, Chorus-Ensemble, Wavetable and Drum Sampler in place, and
# the 12.4 Move banks name Reverb parameters the older banks do not. A name
# of these devices that only the older scripts list may be gone in 12.4.
CHANGED_12_4 = {"Delay", "Chorus2", "InstrumentVector", "DrumCell", "Reverb"}


def rule(fam: str, n: str) -> str:
    """Class code for parameter `n` of a device of family `fam`."""
    s = re.sub(r" \(legacy\)$", "", n.casefold().strip())
    instr = fam in INSTR
    mix = {"reverb": "rm", "delay": "dm", "mod": "mm"}.get(fam, "mix")
    has = lambda p: re.search(p, s)  # noqa: E731
    if has(r"^macro \d+$"):
        return "mac"
    if has(r"^(dry/wet|dry wet|drywet)$"):
        return mix
    # modulation routings: "F1 Freq < Env", "Filt < Vel", "1 Pan <- Random", "Vel > Vol"
    if has(r"(<-?|<) ?(vel|key|random|rnd)\b|<vel|\bvel\.? ?>|^key ?>|\bkb stretch|r < k$|key ?scale"):
        return "sys"
    if has(r"(freq|filt|flt) ?< ?env|^fe amount$|^fe < env$|^filt < env$"):
        return "fe"
    if has(r"< ?lfo|^lfo > |lfo mod amt|^lfo amt [ab]$"):
        return "la"
    mod_amt = r"< ?(pe|env|modwh)\b|mod amt"
    if instr:  # instrument modulation depths
        mod_amt += (r"|^pe amt|^pe amount|pitch env amt|^peg\d amount|mod matrix amt|^global mod amount$"
                    r"|^(mw|pb|press|slide) amt|amount$")
    if has(mod_amt):
        return "ma"
    # switches and choices
    if has(r"^([a-d]e|ae|fe|pe|oe|ve|le|aeg\d|feg\d) loop$"):
        return "sel"
    if has(r"(^| )(on/off|on|off|enabled?|bypass|mute|freeze|frozen|retrig|retrigger|legato|inv|invert|listen|"
           r"audition|link|mono|loop on|sync on|hold on|velocity on)$") or s in ("link", "link channels", "m/s link", "fb inv"):
        return "sw"
    if has(r"(^| )(type|mode|model|wave|waveform|shape|algorithm|structure|routing|style|quality|material|"
           r"circuit|circuit - lp/hp|circuit - bp/no/morph|lp/hp|bp/no/morph|dest [ab]|dest|dst b|destination [ab]|"
           r"source|filter slope|filter \d slope|eq (high|low) slope|"
           r"mono/stereo|precise/retro|peak/rms mode|stereo mode|st mode|time mode|t mode|rate mode|"
           r"sync mode|division|grid|slice by|slice style|unit|interval|choices|sign|base|"
           r"oscillatortype|filtertype|generatortype|transformer\dtype|loopmode|"
           r"pickup model|key priority|glide mode|trigger mode|retrigger mode|fade type)$"):
        return "sel"
    if has(r"(^| )(sync|pb range|note pb range|voices|unison voices|quantize|lookahead|song control|"
           r"tempo control|quantization|monitor|time scaling|auto|lfo sync|l sync|r sync|"
           r"key zone shift|keytrack|error|key error|range)$"):
        return "sys"
    # levels and space
    if has(r"(^| )(pan|panorama|balance)$|^a\d pan|^amp\d pan|pan < "):
        return "pan"
    if has(r"(stereo )?width|spread|stereo image|^wide$|stereo spread|voicespreadamount") and not has(
            r"filter width|flt width|in filter width|tracing width|pinch width|fb width|env width"):
        return "w"
    if has(r"^(volume|global volume|output|output gain|output level|gain|level|master output|trim)$"):
        return "g"
    # filters and tone
    if has(r"(^| )(f\d|filter \d|flt \d|filter|flt|lp|hp|noise filter|cutoff)( freq| frequency|$)|"
           r"filter_frequency|filter freq|flt freq|^lp freq|^hp freq|cutoff"):
        return "cut" if (instr or fam == "filter") else "t"
    if has(r"(^| )(f\d|filter \d|flt \d|filter|flt|lp|hp|noise filter) ?(res|reso|resonance|q)$|^f\d resonance$"):
        return "res" if (instr or fam == "filter") else "o"
    if has(r"\btone\b|color|colour|brightness|warmth|presence|damp|hi ?cut|low ?cut|high-cut|low-cut|hipass|hi pass"):
        return "t"
    # envelopes (effects: attack/release are "other")
    if instr and has(r"(^| |_)(attack|att)$|_times_attack$"):
        return "ea"
    if instr and has(r"(^| |_)(decay)$|_times_decay$"):
        return "ed"
    if instr and has(r"(^| |_)(sustain)$|_sustain$"):
        return "es"
    if instr and has(r"(^| |_)(release|rel)$|_times_release$"):
        return "er"
    # pitch
    if has(r"(^| |_)(transpose|transp|coarse|semi|semitone|octave|oct|tune|pitch|pitch_transpose|pitch st\.)$"):
        return "op" if instr else "p"
    if has(r"(^| |_)(fine|fine tune|detune|pitch_detune)$") and not has(r"unison|uni "):
        return "of" if instr else "p"
    if has(r"unison detune|uni detune|unison amount|^detune$"):
        return "ud"
    if has(r"glide|porta time|glidetime"):
        return "gl"
    # LFOs
    if has(r"(lfo|^l \d|^l |lfo\d).*(rate|speed|freq|frequency|time)|lfo\d? ?s\.? ?rate|lfo1_rate|lfo2_rate|syncedrate|sncrate"):
        return "lr" if instr else "mr"
    if has(r"(lfo|lfo\d).*(amount|amt|depth)"):
        return "la" if instr else "md"
    if has(r"^(drive|crunch|bit depth|sample rate|jitter|saturator drive|shaper drive|filter drive|f\d drive)$|_drive$"):
        return "dr"
    return "o"


# Per-device overrides: "name=code; name=code". Codes: A above.
# Class ties: the resolver binds the first parameter of a class, and the
# LOM order of these devices is not known here, so where two parameters of
# one device would share a class but move the sound in opposite directions
# (low-pass and high-pass cutoff, high and low cut, low and high shelf) the
# one whose raise darkens or thins the sound gets "other" (Drift HP Freq,
# Echo HP Freq, Reverb LowShelf Gain, Tension Body Low-Cut, ...), as a pure
# dry level does.
O = {
    "Amp": "Bass=eg; Middle=eg; Treble=eg; Gain=dr; Volume=g; Dual Mono=sw",
    "AutoFilter": "Slope=sel; Frequency=cut; Resonance=res; Resonance (Legacy)=res; Morph=os; LFO Amount=la; LFO Frequency=lr; LFO Sync Rate=lr;"
                  " Env. Modulation=fe; Env. Attack=o; Env. Release=o; Drive=dr; LFO Offset=o; LFO Phase=o;"
                  " LFO Spin=o; S/C Gain=sys; S/C Mix=sys; LFO Quantize Rate=sel",
    "AutoFilter2": "Frequency=cut; Resonance=res; Control=o; Pitch=o; Formant=o; Filter Morph=os;"
                   " LFO Amount=la; LFO Freq=lr; LFO Rate=lr; LFO 16th=lr; LFO Time=lr; Drive=dr",
    "AutoPan": "Amount=md; Frequency=mr; Sync Rate=mr; Phase=w; Offset=o; Spin=o; Width (Random)=o; Invert=sw",
    "AutoPan2": "Amount=md; Frequency=mr; Rate=mr; Time=mr; 16th=mr; Phase=w; Spin=o; Harmonic=o; Dyn Mod=o;"
                " Attack Time=o; Invert=sw",
    "AutoShift": "Strength=o; Smooth Time=o; Pitch St.=p; Pitch Fine=p; Formant Shift=p; Formant Follow=o;"
                 " Vibrato Amt=md; Vibrato Rate=mr; Root=sel; Scale=sel; Pitch Scale Deg.=p",
    "BeatRepeat": "Chance=o; Decay=o; Filter Freq=t; Filter Width=o; Gate=o; Grid=sel; Interval=sel; Offset=o;"
                  " Pitch=p; Pitch Decay=o; Repeat=sw; Variation=o; Volume=mix; Block Triplets=sw",
    "Cabinet": "Microphone Position=sel; Dual Mono=sw",
    "ChannelEq": "Low Gain=eg; Mid Gain=eg; High Gain=eg; Mid Freq=ef; Gain=g; Output=g; Highpass On=sw",
    "Chorus": "Delay 1 HiPass=o; Delay 1 Time=o; Delay 2 Time=o; Delay 2 Mode=sel; Feedback=md; LFO Amount=md;"
              " LFO Rate=mr; LFO Extend On=sw; Polarity=sel",
    "Chorus2": "Amount=md; Rate=mr; Feedback=md; Offset=o; Gain=g; HP Freq=o; HPF Freq=o; Shape=sel; Warmth=t; Width=w",
    "Compressor2": "Threshold=ca; Ratio=ca; Expansion Ratio=o; Knee=o; Attack=o; Release=o; Makeup=sw;"
                   " Output Gain=u; Model=sel; Env Mode=sel; Auto Release On/Off=sw; S/C EQ Freq=sys;"
                   " S/C EQ Gain=sys; S/C EQ Q=sys; S/C Gain=sys; S/C Mix=sys",
    "Corpus": "Decay=rd; Brightness=t; Bleed=o; Gain=g; Hit=o; Inharmonics=os; Listening L=o; Listening R=o;"
              " Material=os; Opening=o; Radius=o; Ratio=o; Mid Freq=t; Width=w; Spread=w; Tune=p; Fine=p;"
              " Transpose=p; Offset=o; Phase=o; Spin=o; LFO Amount=md; LFO Rate=mr; LFO Sync Rate=mr;"
              " Off Decay=o; Note Off=sw; MIDI Frequency=sw; MIDI Mode=sel; Resonance Type=sel; Resonator Quality=sel",
    "Delay": "L Time=dt; R Time=dt; L 16th=dt; R 16th=dt; L Offset=o; R Offset=o; Feedback=df; Filter Freq=t;"
             " Filter Width=o; Mod Freq=mr; LFO Freq=mr; Dly < Mod=md; LFO > Delay=md; Filter < Mod=md;"
             " LFO > Filter=md; Delay Mode=sel; Ping Pong=sw; Freeze=sw",
    "Drift": "Osc 1 Shape=os; Osc 2 Shape=os; Osc 1 Gain=ol; Osc 2 Gain=ol; Noise Gain=ol; Osc 1 Oct=op;"
             " Osc 2 Oct=op; Osc 2 Detune=of; Transpose=op; Drift=o; LP Freq=cut; HP Freq=o; LP Res=res;"
             " LP Reso=res; Osc 1 Shape Mod Amt=ma; Pitch Mod Amt 1=ma; Pitch Mod Amt 2=ma; LP Mod Amt 1=ma;"
             " LP Mod Amt 2=ma; LFO Amt=la; LFO Mod Amt=la; LFO Rate=lr; LFO Ratio=lr; LFO Synced=lr;"
             " LFO Time=lr; Cyc Env Rate=o; Cyc Env Ratio=o; Cyc Env Synced=o; Cyc Env Time=o; Cyc Env Tilt=o;"
             " Cyc Env Hold=o; Thickness=ud; Spread=w; Strength=ud; Glide Time=gl; Volume=g; Env 2 Cyc On=sw",
    "DrumBuss": "Boom Amt=eg; Boom Decay=o; Boom Freq=ef; Crunch=dr; Damping Freq=t; Drive=dr; Transients=o;"
                " Trim=g; Output Gain=g; Boom Audition=sw; Compressor On=sw",
    "DrumCell": "Start=sp; Length=ed; Decay=ed; Hold=o; Attack=ea; Transpose=op; Detune=of; Filter Freq=cut;"
                " Flt Freq=cut; Filter Res=res; Flt Reso=res; Filter Gain=eg; Pitch Env Amt=ma; Pitch Env Decay=o;"
                " Sub Amt=ol; Sub Freq=o; Noise Amt=ol; Noise Color=t; FM Amt=fm; FM Freq=o; RM Amt=fm; RM Freq=o;"
                " Punch Amt=o; Punch Release=o; 8-Bit Rate=dr; 8-Bit Flt Decay=o; Grain Size=o;"
                " Stretch Factor=o; Loop Length=o; Loop Offset=sp; Mod Amt=sys; Mod Dest=sel; Volume=g; FX Type=sel",
    "Echo": "L Time=dt; R Time=dt; L 16th=dt; R 16th=dt; L Division=sel; R Division=sel; L Offset=o; R Offset=o;"
            " Feedback=df; Feedback Inv=sw; HP Freq=o; LP Freq=t; HP Res=o; LP Res=o; Input Gain=g;"
            " Output Gain=g; Mod Freq=mr; Mod Rate=mr; Dly < Mod=md; Flt < Mod=md; Mod Phase=o; Mod 4x=sw;"
            " Mod Sync=sys; Mod Wave=sel; Env Mix=o; Reverb Level=rm; Reverb Decay=rd; Reverb Loc=sel;"
            " Noise Amt=dr; Noise Mrph=o; Wobble Amt=dr; Wobble Mrph=o; Stereo Width=w; Duck Thr=gate;"
            " Duck Release=o; Gate Thr=gate; Gate Release=o; Clip Dry=sw; Repitch=sw",
    "Eq8": "Adaptive Q=sw; Output Gain=g; Scale=eg",
    "Erosion": "Amount=dr; Frequency=t; Width=o",
    "Erosion2": "Amount=dr; Frequency=t; Filter Width=o; Noise Blend=o",
    "FilterDelay": "Dry=o",
    "FilterEQ3": "GainLo=eg; GainMid=eg; GainHi=eg; FreqLo=ef; FreqHi=ef; LowOn=sw; MidOn=sw; HighOn=sw; Slope=sel",
    "Flanger": "Delay Time=o; Feedback=md; Frequency=mr; Sync Rate=mr; Hi Pass=o; LFO Amount=md;"
               " Env. Modulation=md; Env. Attack=o; Env. Release=o; LFO Offset=o; LFO Phase=o; LFO Spin=o;"
               " LFO Width (Random)=o; Polarity=sel",
    "FrequencyShifter": "Coarse=p; Fine=p; Ring Mod Frequency=p; Drive=dr; LFO Amount=md; LFO Frequency=mr;"
                        " Sync Rate=mr; LFO Offset=o; LFO Phase=o; LFO Spin=o; LFO Width (Random)=o; Wide=w",
    "Gate": "Threshold=gate; Return=o; Floor=gate; Attack=o; Hold=o; Release=o; FlipMode=sw; S/C EQ Freq=sys;"
            " S/C EQ Gain=sys; S/C EQ Q=sys; S/C Gain=sys; S/C Mix=sys",
    "GlueCompressor": "Threshold=ca; Ratio=ca; Range=o; Attack=o; Release=o; Makeup=u; Peak Clip In=sw;"
                      " S/C EQ Freq=sys; S/C EQ Gain=sys; S/C EQ Q=sys; S/C Gain=sys; S/C Mix=sys",
    "GrainDelay": "Beat Delay=dt; Time Delay=dt; Beat Swing=o; Feedback=df; Frequency=o; Pitch=p; Random=o;"
                  " Spray=o; Delay Mode=sel",
    "Hybrid": "Algo Delay=o; Blend=o; Damping=t; Decay=rd; Size=rd; Diffusion=o; Modulation=md; Send Gain=g;"
              " Vintage=sel; EQ High Freq=ef; EQ Low Freq=ef; EQ P1 Freq=ef; EQ P2 Freq=ef; EQ High Gain=eg;"
              " EQ Low Gain=eg; EQ P1 Gain=eg; EQ P2 Gain=eg; EQ P1 Q=eq; EQ P2 Q=eq; EQ High Slope=sel;"
              " EQ Low Slope=sel; EQ Pre Algo=sw; P.Dly Time=o; P.Dly 16th=o; P.Dly Fb Time=o; P.Dly Fb 16th=o;"
              " P.Dly Sync=sys; DH Bass X=o; DH BassMult=o; DH Shape=o; Pr High Mult=o; Pr Low Mult=o;"
              " Pr X Over=o; Qz Distance=o; Qz Low Damp=o; Sh Pitch Shift=p; Sh Shimmer=o; Ti Phase=o;"
              " Ti Rate=mr; Ti Tide=md; Ti Waveform=sel",
    "Limiter": "Gain=ca; Input Gain=ca; Ceiling=g; Release=o; Release time=o; Auto=sw; Lookahead=sys;"
               " Link Channels=sw; M/S Link=sw",
    "Looper": "Feedback=o; Speed=p; Reverse=sw; State=u; Song Control=sys; Tempo Control=sys; Quantization=sys;"
              " Monitor=sys",
    "MultibandDynamics": "Amount=ca; Master Output=g; Low-Mid Crossover=o; Mid-High Crossover=o;"
                         " Peak/RMS Mode=sel; Soft Knee On/Off=sw; Time Scaling=o; S/C Gain=sys; S/C Mix=sys",
    "Overdrive": "Drive=dr; Tone=t; Filter Freq=t; Filter Width=o; Preserve Dynamics=o",
    "Pedal": "Gain=dr; Bass=eg; Mid=eg; Treble=eg; Sub=sw; Mid Freq=sel; Output=g",
    "Phaser": "Color=t; Feedback=md; Frequency=o; LFO Amount=md; LFO Frequency=mr; LFO Sync Rate=mr;"
              " Env. Modulation=md; Env. Attack=o; Env. Release=o; LFO Offset=o; LFO Phase=o; LFO Spin=o;"
              " LFO Width (Random)=o; Poles=o",
    "PhaserNew": "Amount=md; Feedback=md; Center Freq=o; Doubler Time=o; Flange Time=o; Flanger Time=o; Notches=o;"
                 " Warmth=t; Spread=w; Mod Rate=mr; Mod Freq=mr; Mod Rate 2=mr; Mod Freq 2=mr; Mod Sync=sys;"
                 " Mod Sync 2=sys; Mod Phase=o; Mod Blend=o; Lfo Blend=o; Duty Cycle=o; Env Amount=md;"
                 " Env Attack=o; Env Release=o; Safe Freq=o; Output Gain=g; FB Inv=sw; Spin=o",
    "Redux": "Bit Depth=dr; Sample Hard=dr; Sample Soft=dr; Bit On=sw; Sample Mode=sel",
    "Redux2": "Sample Rate=dr; Bit Depth=dr; Jitter=dr; DC Shift=o; Post-Filter=t; Quantizer Shape=o",
    "Resonator": "Decay=rd; Color=t; Const=sw; Frequency=t; Global Gain=g; Width=w; I Note=p; I Tune=p;"
                 " II Pitch=p; II Tune=p; III Pitch=p; III Tune=p; IV Pitch=p; IV Tune=p; V Pitch=p; V Tune=p;"
                 " I Gain=o; II Gain=o; III Gain=o; IV Gain=o; V Gain=o; Mode=sel",
    "Reverb": "Decay Time=rd; Room Size=rd; Predelay=o; Density=o; Diffusion=o; Scale=o; Reflect Level=o;"
              " Diffuse Level=o; ER Spin Amount=o; ER Spin Rate=o; ER Shape=o; Chorus Amount=md; Chorus Rate=mr;"
              " In Filter Freq=o; Input Freq=o; In Filter Width=o; HiFilter Freq=t; Diff. Hi Freq=t;"
              " HiShelf Gain=t; LowShelf Gain=o; LowShelf Freq=o; Size Smoothing=sel; Stereo Image=w;"
              " Freeze On=sw; Flat On=sw; Diff. Hi Type=sel; HiFilter Type=sel",
    "Roar": "Drive=dr; Output=g; Blend=o; Comp Amt=ca; Tone Amt=t; Tone Freq=t; Global Mod Amt=ma;"
            " Shaper 1 Amt=dr; Shaper 2 Amt=dr; Shaper 3 Amt=dr; Shaper 1 Bias=dr; Shaper 2 Bias=dr;"
            " Shaper 3 Bias=dr; Shaper 1 Level=g; Shaper 2 Level=g; Shaper 3 Level=g;"
            " Flt 1 Freq=cut; Flt 2 Freq=cut; Flt 3 Freq=cut; Flt 1 Res=res; Flt 2 Res=res; Flt 3 Res=res;"
            " Flt 1 Morph=o; Flt 2 Morph=o; Flt 3 Morph=o; Flt 1 Peak=o; Flt 2 Peak=o; Flt 3 Peak=o;"
            " Fb Amt=df; Fb Time=dt; Fb Synced=dt; Fb Note=p; Fb Freq=t; Fb Width=o;"
            " LFO 1 Rate=mr; LFO 2 Rate=mr; LFO 1 Synced Rate=mr; LFO 2 Synced Rate=mr; LFO 1 16th=mr;"
            " LFO 2 16th=mr; LFO 1 Morph=o; LFO 2 Morph=o; LFO 1 Smooth=o; LFO 2 Smooth=o; Noise Rate=mr;"
            " Noise Synced Rate=mr; Noise 16th=mr; Noise Smooth=o; Env Attack=o; Env Release=o; Env Thresh=o;"
            " Env Gain=o; Env Freq=o; Env Width=o; Low Mid X-Over=o; Mid High X-Over=o",
    "Saturator": "Drive=dr; Base=o; Color=sw; Color Amt Hi=t; Color Amt Low=o; Color Freq=o; Color Width=o;"
                 " Depth=t; Frequency=o; Width=o; Output=g; Soft Clip=sw; WS Curve=o; WS Damp=o; WS Depth=o;"
                 " WS Drive=dr; WS Lin=o; WS Linearity=o; WS Period=o",
    "Shifter": "Pitch Coarse=p; Pitch Fine=p; FShift Coarse=p; RM Coarse=p; Mod Fine=p; RM Drive Gain=dr;"
               " Delay Time=dt; Delay S. Time=dt; Delay Feedback=df; Delay Sync=sys; Env Amount Hz=md;"
               " Env Amount St=md; Env Attack=o; Env Release=o; Lfo Amount Hz=md; Lfo Amount St=md;"
               " Lfo Rate Hz=mr; Lfo S. Rate=mr; Lfo Duty Cycle=o; Lfo Offset=o; Lfo Phase=o; Lfo Spin Amount=o;"
               " Lfo Width=o; MidiPitch Glide=o; Pitch Window=o; Tone=t; Wide=w",
    "Spectral": "Delay Mix=dm; Delay Feedback=df; Delay Time Seconds=dt; Delay Time Sixteenths=dt;"
                " Delay Time Divisions=dt; Delay Frequency Shift=p; Delay Tilt=t; Delay Spray=o;"
                " Delay Mask=o; Delay Stereo Spread=w; Delay Dly. Unit=sel; Delay On=sw; Frozen=sw; On=sw;"
                " Fade In=o; Fade Out=o; Sensitivity=o; S.Rate ms=o; Sync Interval=sel; XFade %=o;"
                " Input Send Gain=g",
    "StereoGain": "Gain=u; Gain (Legacy)=u; Balance=pan; Stereo Width=w; Mid/Side Balance=w; Bass Freq=o;"
                  " Bass Mono=sw; DC Filter=sw; Mono=sw; Mute=sw; Left Inv=sw; Right Inv=sw; Channel Mode=sel",
    "Transmute": "Decay=rd; Freq. Hz=p; Note=p; Transpose=p; Shift=p; Stretch=o; Harmonics=o; HF Damp=t;"
                 " LF Damp=o; Glide=o; Mod Rate=mr; Pitch Mod=md; Unison=o; Unison Amount=ud; Input Send Gain=g",
    "Tube": "Drive=dr; Bias=dr; Tone=t; Envelope=o; Attack=o; Release=o; Output=g",
    "Vinyl": "Tracing Drive=dr; Pinch Drive=dr; Global Drive=dr; Crackle Density=o; Crackle Volume=o;"
             " Tracing Freq.=t; Pinch Freq.=t; Tracing Width=o; Pinch Width=o; Pinch Mono On=sw;"
             " Pinch Soft On=sw",
    "Vocoder": "Formant Shift=p; Attack Time=o; Release Time=o; Output Level=g; Gate Threshold=gate;"
               " Envelope Depth=md; Filter Bandwidth=o; Upper Filter Band=t; Lower Filter Band=o; Unvoiced Level=o;"
               " Unvoiced Sensitivity=o; Unvoiced Speed=o; Enhance=sw; Ext. In Gain=g; Noise Crackle=o;"
               " Noise Rate=o; Oscillator Pitch=p; Lower Pitch Detection=sys; Upper Pitch Detection=sys",
    # instruments
    "Collision": "Mallet Volume=ol; Mallet Stiffness=t; Mallet Noise Amount=o; Mallet Noise Color=t;"
                 " Noise Volume=ol; Noise Filter Freq=cut; Noise Filter Q=res; Res 1 Decay=ed; Res 2 Decay=ed;"
                 " Res 1 Brightness=t; Res 2 Brightness=t; Res 1 Inharmonics=os; Res 2 Inharmonics=os;"
                 " Res 1 Material=os; Res 2 Material=os; Res 1 Tune=op; Res 2 Tune=op; Res 1 Fine Tune=of;"
                 " Res 2 Fine Tune=of; Res 1 Pitch Env.=ma; Res 2 Pitch Env.=ma; Res 1 Pitch Env. Time=o;"
                 " Res 2 Pitch Env. Time=o; Res 1 Volume=ol; Res 2 Volume=ol; Res 1 Bleed=o; Res 2 Bleed=o;"
                 " Res 1 Hit=o; Res 2 Hit=o; Res 1 Listening L=o; Res 1 Listening R=o; Res 2 Listening L=o;"
                 " Res 2 Listening R=o; Res 1 Radius=o; Res 2 Radius=o; Res 1 Opening=o; Res 2 Opening=o;"
                 " Res 1 Ratio=o; Res 2 Ratio=o; Res 2 Panorama=pan; Structure=sel; LFO 1 Depth=la; LFO 2 Depth=la;"
                 " LFO 1 Amt A=la; LFO 1 Amt B=la; LFO 2 Amt A=la; LFO 2 Amt B=la; LFO 1 Destination A Amount=la;"
                 " LFO 2 Destination A Amount=la; LFO 1 Offset=o; LFO 2 Offset=o; LFO 1 Rate=lr; LFO 2 Rate=lr;"
                 " LFO 1 Sync Rate=lr; LFO 2 Sync Rate=lr; MW Amt A=ma; MW Amt B=ma; PB Amt A=ma; Press Amt A=ma;"
                 " Press Amt B=ma; Slide Amt A=ma; Slide Amt B=ma",
    "InstrumentImpulse": "Global Time=o; Global Transpose=op; Global Volume=g",
    "InstrumentMeld": "MeldVoice_Drive=dr; MeldVoice_EngineBDelay=o; MeldVoice_VoiceSpreadAmount=w; Volume=g",
    "InstrumentVector": "Osc 1 Pos=os; Osc 2 Pos=os; Osc 1 Gain=ol; Osc 2 Gain=ol; Sub Gain=ol; Osc 1 Transp=op;"
                        " Osc 2 Transp=op; Sub Transpose=op; Osc 1 Detune=of; Osc 2 Detune=of; Osc 1 Effect 1=os;"
                        " Osc 1 Effect 2=os; Osc 2 Effect 1=os; Osc 2 Effect 2=os; Sub Tone=os; Osc 1 Pan=pan;"
                        " Osc 2 Pan=pan; Filter 1 Morph=os; Filter 2 Morph=os; Filter 1 Drive=dr; Filter 2 Drive=dr;"
                        " Env 2 Attack=ea; Env 3 Attack=ea; Env 2 Decay=ed; Env 3 Decay=ed; Env 2 Sustain=es;"
                        " Env 3 Sustain=es; Env 2 Release=er; Env 3 Release=er; Env 2 Initial=o; Env 3 Initial=o;"
                        " Env 2 Peak=o; Env 3 Peak=o; Env 2 Final=o; Env 3 Final=o; LFO 1 Amount=la; LFO 2 Amount=la;"
                        " LFO 1 Rate=lr; LFO 2 Rate=lr; LFO 1 S. Rate=lr; LFO 2 S. Rate=lr; LFO 1 Shaping=o;"
                        " LFO 2 Shaping=o; LFO 1 Attack Time=o; LFO 2 Attack Time=o; LFO 1 Phase Offset=o;"
                        " LFO 2 Phase Offset=o; Unison Amount=ud; Global Mod Amount=ma; Time=o; Glide=gl;"
                        " Internal Filter=sw; Transpose=op",
    "LoungeLizard": "M Stiffness=t; M Force=o; Noise Pitch=o; Noise Decay=o; Noise Amount=ol; F Tine Color=t;"
                    " F Tine Decay=ed; F Tine Vol=ol; F Tone Decay=ed; F Tone Vol=ol; F Release=er; Damp Tone=t;"
                    " Damp Balance=o; Damp Amount=o; P Symmetry=o; P Distance=o; P Amp In=dr; P Amp Out=g;"
                    " Semitone=op; Detune=of",
    "MultiSampler": "Time=o; Spread=ud; O Coarse=op; O Fine=of; O Volume=fm; O Fix Freq=o; O Fix Freq Mul=o;"
                    " O Fix On=sw; Shaper Amt=dr; Ae Init=o; Ae Peak=o; Ae End=o; Fe Init=o; Fe Peak=o; Fe End=o;"
                    " Oe Init=o; Oe Peak=o; Oe End=o; Pe Init=o; Pe Peak=o; Pe End=o; Ve Init=o; Ve Peak=o;"
                    " L 1 Offset=o; L 2 Offset=o; L 3 Offset=o; L 2 Phase=o; L 3 Phase=o; L 2 Spin=o; L 3 Spin=o;"
                    " L 1 Attack=o; L 2 Attack=o; L 3 Attack=o; Filter Morph=os; Detune=of",
    "Operator": "Time=o; Tone=t; Spread=ud; Fe Amount=fe; Pe Amount=ma; Pe Init=o; Pe Peak=o; Pe End=o;"
                " Ae Init=o; Ae Peak=o; Be Init=o; Be Peak=o; Ce Init=o; Ce Peak=o; De Init=o; De Peak=o;"
                " Fe Init=o; Fe Peak=o; Fe End=o; Le Init=o; Le Peak=o; Le End=o; Le Attack=o; Le Decay=o;"
                " Le Sustain=o; Le Release=o; Osc-A Level=ol; Osc-B Level=fm; Osc-C Level=fm; Osc-D Level=fm;"
                " Osc-A Phase=o; Osc-B Phase=o; Osc-C Phase=o; Osc-D Phase=o; Shaper Drive=dr; Shaper Mix=o;"
                " LFO Amt=la; LFO Rate=lr; Filter Morph=os; Panorama=pan",
    "OriginalSimpler": "S Start=sp; S Length=o; S Loop Length=o; S Loop Fade=o; Fade In=o; Fade Out=o; Nudge=o;"
                       " Regions=sel; Spread=ud; Filter Morph=os; Detune=of; L Attack=o; L Offset=o; L Rate=lr;"
                       " L Sync Rate=lr",
    "StringStudio": "Volume=g; Octave=op; Semitone=op; Fine Tune=of; Stretch=o; Filter Freq=cut; Filter Reso=res;"
                    " Body Mix=o; Body Decay=o; Body Size=o; Body High-Cut=t; Body Low-Cut=o; String Decay=ed;"
                    " Str Damping=t; Str Inharmon=os; S Decay Ratio=o; Exc Velocity=o; Exc Damping=t;"
                    " Exc FricStiff=t; Exc ForceMassProt=o; Damper Mass=o; D Stiffness=t; D Damping=o;"
                    " D Velocity=o; Damp Pos=o; D Pos Abs=o; E Pos=o; E Pos Abs=o; Pickup Pos=t; Term Mass=o;"
                    " Term Fng Stiff=o; Term Fret Stiff=o; Error=sys; Vib Amount=la; Vib Speed=lr;"
                    " Vib Delay=o; Vib Fade-In=o; Vib Error=o; LFO Speed=lr; LFO SyncRate=lr; LFO Delay=o;"
                    " LFO Fade In=o; FEG Attack=ea; FEG Decay=ed; FEG Sustain=es; FEG Release=er;"
                    " Uni Delay=o; Porta Time=gl; Porta Prop=sw; Porta Legato=sw; Damper Gated=sw;"
                    " Press Amt A=ma; Press Amt B=ma; Slide Amt A=ma; Slide Amt B=ma; Key Dest=sel; Vel Dest=sel",
    "UltraAnalog": "OSC1 Shape=sel; OSC2 Shape=sel; OSC1 PW=os; OSC2 PW=os; OSC1 Level=ol; OSC2 Level=ol;"
                   " OSC1 Balance=o; OSC2 Balance=o; AMP1 Level=ol; AMP2 Level=ol; Noise Level=ol;"
                   " Noise Balance=o; Noise Color=t; Detune=of; Octave=op; Semitone=op; OSC1 Detune=of;"
                   " OSC2 Detune=of; O1 Sub/Sync=fm; O2 Sub/Sync=fm; F1 Drive=dr; F2 Drive=dr; PEG1 Amount=ma;"
                   " PEG2 Amount=ma; PEG1 Time=o; PEG2 Time=o; LFO1 PW=o; LFO2 PW=o; LFO1 Phase=o; LFO2 Phase=o;"
                   " LFO1 Delay=o; LFO2 Delay=o; LFO1 Fade In=o; LFO2 Fade In=o; AEG1 S Time=o; AEG2 S Time=o;"
                   " FEG1 S Time=o; FEG2 S Time=o; Unison Delay=o; Vib Amount=la; Vib Speed=lr; Vib Delay=o;"
                   " Vib Fade-In=o; Vib Error=o; Press Amt A=ma; Press Amt B=ma; Slide Amt A=ma; Slide Amt B=ma;"
                   " OSC1 Mode=sel; OSC2 Mode=sel",
    # MIDI effects: nothing here is an audio quantity; rates and
    # depths of the note generators are "other"
    "MidiArpeggiator": "Free Rate=o; Synced Rate=o; Gate=o; Groove=o; Offset=o; Repeats=o; Transp. Dist.=o;"
                       " Transp. Steps=o; Tranpose Key=sel; Velocity Decay=o; Ret. Interval=sel; Style=sel",
    "MidiCcControl": " ".join(f"Custom {c}=mac;" for c in "ABCDEFGHIJKLM") + " Mod Wheel=ma; Pitch Bend=o; Pressure=o",
    "MidiChord": " ".join(f"Shift{i}=o; Velocity{i}=o;" for i in range(1, 7)),
    "MidiNoteLength": "Time Length=o; Synced Length=o; Gate=o; Decay Time=o; Decay Key Scale=o;"
                      " Release Velocity=o; Latch=sw; Trigger Source=sel",
    "MidiPitcher": "Pitch=op; Lowest=sys; Range=sys",
    "MidiRandom": "Chance=o; Choices=o; Interval=o; Sign=sel; Mode=sel",
    "MidiScale": "Base=sel; Transpose=op; Lowest=sys; Range=sys; Fold=sw",
    "MidiVelocity": "Drive=o; Compand=o; Random=o; Out Hi=o; Out Low=o; Lowest=sys; Range=sys; Mode=sel",
}

# Pattern overrides for numbered or prefixed families: (class_name, regex, code)
P = [
    ("Eq8", r"^\d (gain) [ab]$", "eg"), ("Eq8", r"^\d frequency [ab]$", "ef"), ("Eq8", r"^\d resonance [ab]$", "eq"),
    ("Eq8", r"^\d filter type [ab]$", "sel"), ("Eq8", r"^\d filter on [ab]$", "sw"),
    ("FilterDelay", r"^\d (beat|time) delay$", "dt"), ("FilterDelay", r"^\d feedback$", "df"),
    ("FilterDelay", r"^\d filter freq$", "t"), ("FilterDelay", r"^\d filter width$", "o"),
    ("FilterDelay", r"^\d volume$", "dm"), ("FilterDelay", r"^\d beat swing$", "o"),
    ("FilterDelay", r"^\d pan$", "pan"), ("FilterDelay", r"^\d input on$", "sw"),
    ("FilterDelay", r"^\d delay mode$", "sel"),
    ("MultibandDynamics", r"^(above|below) threshold", "ca"), ("MultibandDynamics", r"^above ratio", "ca"),
    ("MultibandDynamics", r"^below ratio", "o"), ("MultibandDynamics", r"^(attack|release) time", "o"),
    ("MultibandDynamics", r"^input gain", "g"), ("MultibandDynamics", r"^output gain", "g"),
    ("MultibandDynamics", r"^band activator", "sw"),
    ("InstrumentImpulse", r"^\d start$", "sp"), ("InstrumentImpulse", r"^\d transpose$", "op"),
    ("InstrumentImpulse", r"^\d stretch factor$", "o"), ("InstrumentImpulse", r"^\d saturator drive$", "dr"),
    ("InstrumentImpulse", r"^\d filter freq$", "cut"), ("InstrumentImpulse", r"^\d filter res$", "res"),
    ("InstrumentImpulse", r"^\d volume$", "ol"), ("InstrumentImpulse", r"^\d envelope decay$", "ed"),
    ("Operator", r"^[a-d] coarse$", "op"), ("Operator", r"^[a-d] fine$", "of"),
    ("Operator", r"^[a-d] fix freq( mul)?$", "o"), ("Operator", r"^[a-d] quantize$", "sw"),
    ("Operator", r"^osc-[a-d] wave$", "sel"),
    ("Collision", r"^lfo \d (dest|destination) [ab]$", "sel"),
    ("Collision", r"^res \d (quality|type)$", "sel"),
    ("InstrumentMeld", r"_slopes_", "o"), ("InstrumentMeld", r"_times_attack$", "ea"), ("InstrumentMeld", r"_times_decay$", "ed"),
    ("InstrumentMeld", r"_(sustain|values_sustain)$", "es"), ("InstrumentMeld", r"_times_release$", "er"),
    ("InstrumentMeld", r"_loopmode$", "sel"),
    ("InstrumentMeld", r"_filter_frequency$", "cut"), ("InstrumentMeld", r"_filter_macro\d$", "o"),
    ("InstrumentMeld", r"_filter_filtertype$", "sel"), ("InstrumentMeld", r"_oscillator_macro\d$", "os"),
    ("InstrumentMeld", r"_oscillator_oscillatortype$", "sel"), ("InstrumentMeld", r"_pitch_transpose$", "op"),
    ("InstrumentMeld", r"_pitch_detune$", "of"), ("InstrumentMeld", r"_tonefilter$", "t"),
    ("InstrumentMeld", r"_engine[ab]_pan$", "pan"), ("InstrumentMeld", r"_engine[ab]_volume$", "ol"),
    ("InstrumentMeld", r"_glidetime$", "gl"), ("InstrumentMeld", r"_lfo\d_(rate|syncedrate)$", "lr"),
    ("InstrumentMeld", r"_lfo\d_sync$", "sys"), ("InstrumentMeld", r"_lfo1_(generator|transformer\d)macro\d?$", "o"),
    ("InstrumentMeld", r"_lfo1_(generatortype|transformer\dtype)$", "sel"), ("InstrumentMeld", r"_lfo2_waveform$", "sel"),
    ("InstrumentMeld", r"_lfo1_phaseoffset$", "o"),
]


def overrides():
    out = {}
    for cls, spec in O.items():
        for item in filter(None, (x.strip() for x in spec.split(";"))):
            name, code = item.rsplit("=", 1)
            assert code in A, (cls, item)
            out[(cls, name.strip())] = code
    return out


def manual():
    """(class_name, name) -> "yes"/"no" from manual_12_4.tsv."""
    out = {}
    for line in (HERE / "manual_12_4.tsv").read_text(encoding="utf-8").splitlines()[1:]:
        cls, name, hit = line.split("\t")
        out[(cls, name)] = hit
    return out


def main() -> None:
    ov = overrides()
    man = manual()
    in_manual = {c for c, _ in man} | {"Erosion"}  # "now named Erosion Legacy" (manual 29.17)
    used = set()
    rows = []
    for line in (HERE / "lom_names.tsv").read_text(encoding="utf-8").splitlines()[1:]:
        cls, name, src = line.split("\t")
        if cls not in DEVICES:
            continue
        disp, fam = DEVICES[cls]
        code = ov.get((cls, name))
        if code:
            used.add((cls, name))
        else:
            for c, pat, cd in P:
                if c == cls and re.search(pat, name.casefold()):
                    code = cd
                    break
            else:
                code = rule(fam, name)
        c = A[code]
        # a pure dry level keeps no mix class: on a class tie the resolver
        # binds the first parameter (tools/mapping/fl/stock_labels.py)
        if re.search(r"\bdry\b", name, re.I) and not re.search(r"wet", name, re.I) and c in MIXES:
            c = "other"
        tags = src.split(",")
        hit = man.get((cls, name), "-")
        if "move-12.4" in tags or hit == "yes":
            status = "12.4"
        elif name == "Device On":
            status = "12.1"
        elif cls in CHANGED_12_4:
            status = "UNVERIFIED in 12.4"
        else:
            status = "12.1" if any(t.endswith("12.1") for t in tags) else "12.0"
        dm = "yes" if cls in in_manual else "no"
        rows.append(f"ableton:{cls}\t{cls}\t{disp}\t{dm}\t{name}\t{c}\t{status}\t{hit}\t{src}")
    stale = set(ov) - used
    assert not stale, sorted(stale)
    print("plugin\tclass_name\tdisplay_name\tdisplay_in_manual\tname\tclass\tstatus\tmanual\tsources")
    print("\n".join(rows))


if __name__ == "__main__":
    sys.exit(main())
