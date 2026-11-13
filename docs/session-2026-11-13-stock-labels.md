# Session 2026-11-13: stock labels for Ableton Live and Logic Pro

Cloud session (Linux container, no DAW), branch `claude/gifted-bardeen-27z59j`,
merged as PR 9 (`786c872`, merge record 57). Handoff for the bridge work:
`docs/handoff-2026-11-13-bridges.md`.

## Built

- `tools/mapping/ableton/`: LOM parameter names of 71 Live devices (2,421
  names) from the parameter banks in Live's MIDI Remote Scripts, read from
  two decompiled mirrors with `ast`; cross-check against the Live 12.4
  Reference Manual; hand labels by family rules plus overrides.
- `tools/mapping/logic/`: control names of 87 Logic Pro 13 plug-ins (1,744
  names) from Apple's Logic Pro User Guide 13.0; hand labels with the same
  rules plus Logic rules.
- `tools/mapping/stock.py` (all stock tables), `merge_known.py` (writes them
  into `known.tsv` without `data/` or torch), `test_stock.py` (9 tests).
- `known.tsv` 943 -> 1,101 plugins, keys `ableton:<class_name>` and
  `logic:<plug-in>`, every existing line byte-identical.
- 0.1.3 in `CHANGELOG.md` and the README stock table.
- `tools/mapping/fl/stock_names.py`: exports the names of the FL stock
  plugins without hand labels from the FL dump, so they can be labelled away
  from the Mac.

## Measured / found

Numbers and sources: `probes/ableton-stock/RESULT.md`,
`probes/logic-stock/RESULT.md`.

- Live 12.4.7 is current (2026-10-22). The decompiled script mirrors are
  mostly Live 12.0 (bytecode compiled 2024-03); only the Python 3.11 files
  (Move banks) are 12.4. 544 names confirmed for 12.4, 124 UNVERIFIED for
  devices 12.4 changed.
- Live's scripts look parameters up by `original_name`
  (`device_parameter_bank.py`) and find a device's on switch by
  `original_name.startswith("Device On")`. Meld's `original_name`s are
  `MeldVoice_*` ids.
- Logic Pro 13 came out 2026-11-05; its guide is 13.0 for macOS 27. Logic's
  own plug-ins have no machine-readable parameter list; the guide's control
  labels may differ from Logic's automation names.
- Bug, not fixed: Kontakt's parameters `#000`... break `sense.rs`'s
  `known.tsv` parser (a line starting with `#` is read as a plugin). Queued
  as a separate task ("Fix known.tsv parsing, then slim the code").
- Coverage of stock plugins by labels: FL 56 of 106 with parameters (53 %),
  Live 63 of 67 current devices (94 %), Logic 87 of about 113 own plug-ins
  (about 77 %, counted from the guide's table of contents). Working in the
  DAW today: FL only.

## Not run

- `cargo test -p synthexis` (the plugin builds on Apple only). The
  `known.tsv` parser of `sense.rs` was replayed in Python. Run on the Mac:
  `cd plugin && ./guarded-cargo.sh test -p synthexis`.
- The full `export_paramsense.py` (needs `data/`); its stock part was
  checked with stubbed torch.

## Environment

The cloud environment "synthexis" (personal) allows www.ableton.com,
support.apple.com and www.image-line.com besides the package registries.
