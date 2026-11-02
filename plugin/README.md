# synthexis plugin (Welle 3 skeleton)

Camera-to-parameter macro plugin. 32 "Signal N" FloatParams are driven by
the shared perception engine (built-in camera: brightness + motion, plus a
30 Hz test.triangle) through the cond chain (range, One Euro, occlusion,
reacquire slew) and the route table, then written to the host via a
coalesced main-queue dispatch of begin/set/end parameter gestures — the H2
write path (FL Studio 2026 records main-thread VST3 edits with the editor
closed). Audio is stereo silence; perception never runs on the audio thread.

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
