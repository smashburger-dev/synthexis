# Vendored nih-plug patches

Base: nih-plug @ de421011f41a6d10fc8c7a6084e4f4dee0143683 (2026-10-26
master), vendored from ~/.cargo/git/checkouts. Kept crates: nih_plug (root),
nih_plug_derive, nih_plug_egui, nih_plug_xtask + LICENSE files. Dropped:
plugins/, nih_plug_iced, nih_plug_vizia, cargo_nih_plug, xtask, and the
workspace members list was trimmed accordingly.

## host_context_available hook (why)

nih-plug only hands out a `GuiContext` when the editor spawns, but FL Studio
2026 records VST3 parameter edits issued on the main thread even with the
plugin window closed (H2, see probes/fl-camera-plugin/RESULT.md). To write
automation without an editor we need the context as soon as the host gives
it, not when a window opens.

## Diff

- `src/plugin.rs`: trait `Plugin` gains
  `fn host_context_available(&mut self, _context: Arc<dyn GuiContext>) {}`
  (default no-op); `GuiContext` added to the prelude import.
- `src/wrapper/vst3/wrapper.rs` `set_component_handler`: after storing the
  handler, if one is now set, calls
  `plugin.lock().host_context_available(inner.clone().make_gui_context())`.
- `src/wrapper/clap/wrapper.rs` `init`: after the host extension queries,
  if `host_params` exists, clones the leaked wrapper Arc
  (`Arc::from_raw`/`into_raw` pair) and calls the hook with
  `make_gui_context()`.
- `src/wrapper/standalone/wrapper.rs` `run`: after `make_gui_context()`,
  calls the hook.
