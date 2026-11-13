# FL Studio parameter harvest

Dumps every plugin (channel rack + mixer slots) of an FL project with all
parameter names, values and display strings, in the JSON layout
`import_fl_dump.py` reads (`{"ts", "plugins": [{"location", "name",
"user_name", "n_params", "params": [{"index", "name", "value", "samples"}]}]}`).

FL 2026 (script API 45) gives device scripts no file writes, so
`device_synthexis_harvest.py` can no longer write the dump itself. It is now
the dump code that runs inside FL: `probes/fl-stock-plugins/run_stock.py`
execs it through the probe-only dev script and fetches `_dump_one` page by
page over MIDI. Steps and gotchas: `probes/fl-stock-plugins/README.md`.

The stock dump of 2026-11-13 (109 plugins, 16,160 parameters):

```bash
uv run probes/fl-stock-plugins/fldev.py serve &      # dev script installed, then start FL
uv run --with python-rtmidi==1.5.8 python probes/fl-stock-plugins/run_stock.py
uv run tools/mapping/fl/import_fl_dump.py ~/Documents/synthexis-harvest/<dump>.json
```

`import_fl_dump.py` writes `data/mapping/harvest/fl/{plugins,params}.jsonl`
for `label.py --harvest fl --priority`. Until that paid run happens, the
stock effects with up to 60 parameters carry hand labels
(`stock_labels.py` -> `stock_labels.tsv`, read by `export_paramsense.py`).

The dump samples every parameter at 0, 0.2, 0.5, 0.8 and 1 and puts it
back, so only run it on a throwaway project. Params 4096+ (FL's per-CC
entries) are skipped. Plugins whose parameters the script API cannot reach
land with `n_params: 0` (Patcher, Wave Traveller, Soundgoodizer).
