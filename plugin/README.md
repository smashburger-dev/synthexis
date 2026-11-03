# synthexis plugin (Welle 3 skeleton)

Camera-to-parameter macro plugin. 32 "Signal N" FloatParams are driven by
the shared perception engine (built-in camera: brightness + motion, plus a
30 Hz test.triangle) through the cond chain (range, median of 3, One Euro,
occlusion, reacquire slew) and the route table, then written to the host via a
coalesced main-queue dispatch of begin/set/end parameter gestures — the H2
write path (FL Studio 2026 records main-thread VST3 edits with the editor
closed). Audio is stereo silence; perception never runs on the audio thread.
The built-in routes (brightness, motion, test.triangle, perception on
Signal 3..10) are test routes: they move macros only while the "Test
signal" parameter is on. With it off, only assigned routes move macros, so
FL records only those.

## Commands

```
./guarded-cargo.sh test -p synthexis      # unit tests (cond, route)
./guarded-cargo.sh test -p synthexis-parser --release -- --include-ignored
                                          # parser parity (needs data/mapping/runtime)
./guarded-cargo.sh clippy -p synthexis --release
./guarded-cargo.sh xtask bundle synthexis --release
./install.sh                              # build+sign+install (see FL note)
SYNTHEXIS_TEST="test_signal=on" \
  target/bundled/synthexis.app/Contents/MacOS/synthexis -r 44100
```

install.sh needs Apple's Command Line Tools and Rust. When one is missing
it says so and offers the official installer (`xcode-select --install`,
rustup from https://rustup.rs) and installs only after a yes; without a
terminal it stops with the command to run. If `coremlcompiler` cannot
compile a model, the .mlpackage is copied and the plugin compiles it when
it loads.

Last, install.sh runs `assistant/install.sh` for the mapping assistant's
parser and completion data (dev links or the release download); if that
fails, the plugin installs without the sentence field.

install.sh refuses only when OsxFL is running AND the synthexis bundle is
already installed (a loaded dylib can't be replaced safely; first install
is fine — FL hasn't loaded it). Log: ~/Library/Logs/synthexis.log.

Mapping assistant parser (`parser/`, crate synthexis-parser): loads from
$SYNTHEXIS_PARSER_DIR or ~/Library/Application Support/synthexis/parser
(export: `uv run tools/mapping/export_runtime.py --run
p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3 --rules conv,pronoun --q8`;
$SYNTHEXIS_PARSER_RUNS picks other runs);
`SYNTHEXIS_PARSE="sentence"` logs that sentence's frame once loaded. See
docs/parser-runtime-plan.md.

Sentence field completion (`complete/`, crate synthexis-complete): data from
$SYNTHEXIS_COMPLETION_DIR or ~/Library/Application Support/synthexis/completion
(export: `uv run tools/mapping/export_completion.py`). Chips become pinned
spans on the parser's frame (`parser/src/pin.rs`). See
docs/mapping-completion.md.

## Perception

`synthexis/src/engine/perception.rs` runs on its own thread, fed by the
camera callback through a one-slot channel (a busy thread drops frames,
never queues them). Backend `students`: seg student (384x216, previous
mask fed back) -> hand and face boxes from the mask -> 128 px crops ->
hand and face students -> `feat.*` signals; backend `vision`: Apple Vision
hand pose and face landmarks into the same geometry (`engine/geom.rs`).
Signal ids are `cam0.` + the ontology source id with the side after the
first segment (`cam0.hand.R.openness`, `cam0.mouth.open`); a unit test
checks them against `tools/mapping/ontology.yaml`.

`install.sh` compiles the students into
`~/Library/Application Support/synthexis/models` (from `data/models/` when
it exists, else the tracked copies in `plugin/models/`, else the preflight
stand-ins) and writes `backend` there: `hybrid` by default: the students
for seg and hands, Apple Vision face landmarks on the seg face box
(probes/plugin-perception/RESULT.md). When the landmarks cannot start or
fail 30 frames in a row, `hybrid` takes the face from the face student
(mouth usable, eyes weak). Override with
`SYNTHEXIS_PERCEPTION=students|vision|off`.

Offline bench over a recording (no camera needed):

```
SYNTHEXIS_BENCH=../data/recordings/2026-11-01-S4/cam0.mov \
  SYNTHEXIS_BENCH_OUT=/tmp/s4.jsonl target/release/synthexis
python3 ../probes/plugin-perception/eval.py /tmp/s4.jsonl \
  ../data/recordings/2026-11-01-S4/segments.json
```

Numbers: `probes/plugin-perception/RESULT.md`; where the tuning knobs are and how to change them: `probes/plugin-perception/TUNING.md`.

## Vendored nih-plug patch

plugin/vendor/nih-plug is nih-plug @ de42101 with one additive hook:
`Plugin::host_context_available(Arc<dyn GuiContext>)`, fired by the VST3
wrapper on set_component_handler, the CLAP wrapper after init() gets
host_params, and the standalone wrapper on context creation — so the
writer gets a GuiContext without an editor window. See vendor/PATCHES.md.

## Licence note

nih-plug's VST3 bindings are GPLv3: distributed **VST3 binaries** of this
plugin are GPLv3. Our own source stays MIT (the probe and this crate are
hackathon work under repo rules). CLAP builds are unaffected by that
linking constraint.
