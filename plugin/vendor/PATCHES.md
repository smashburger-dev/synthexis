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

## Diagnostics (synthexis-nih.log)

- `src/wrapper/vst3/wrapper.rs`: `nih_diag_log`/`nih_param_name`/
  `nih_last_gets` helpers appending to `~/Library/Logs/synthexis-nih.log`;
  logs every `set_param_normalized` (hash, name, value, is_processing,
  applied), `set_active`/`set_processing` transitions, input_param_changes
  queues inside `process()` (hash, name, count, last value — only when
  queues exist), and `get_param_normalized` only when the read-back value
  differs from the last logged. Diagnostics only, no behaviour change.
  Diagnostics are gated on the marker file
  `~/Library/Logs/synthexis-nih.enable` (checked once per process via
  OnceLock): without the file, nih_diag_log is a no-op, so the ~60 lines/s
  echo of our own automation writes stops.

## Plug-in-initiated window resize (ui-redesign, 2026-11-04)

Unused since v3 sizing (docs/design/ui-redesign.md 2.11): FL does not honour
a plug-in's resize, so synthexis no longer calls `set_requested_size`; the
window keeps one fixed size. The patches stay harmless and in place.

The redesigned window asks the host for its own size (docs/design/ui-redesign.md,
section 2.8). Upstream nih_plug_egui keeps the resize path private and resizes
the wrong way, so three patches. Each patched spot carries a
`synthexis patch` comment.

- `nih_plug_egui/src/lib.rs`: `EguiState::set_requested_size` becomes `pub`.
  Upstream only lets the `ResizableWindow` corner widget call it; the plug-in
  needs it to ask for the size its layout plans.
- `nih_plug_egui/src/editor.rs`: the resize step moves into
  `take_resize_request`. Upstream took the request (`requested_size.swap(None)`),
  called `context.request_resize()` and stored the new size only afterwards.
  The wrappers read `Editor::size()` from inside `request_resize()`: the VST3
  wrapper on macOS calls the host's `resizeView` synchronously when it runs on
  the GUI thread (`vst3/view.rs` `request_resize`), and the standalone queues
  `GuiTask::Resize(size())` (`standalone/wrapper.rs` `request_resize`). Both
  got the old size, so the host resized to the size it already had. Now the
  new size is stored first and the old one restored when `request_resize()`
  returns false (CLAP passes the host's answer through; VST3 and the
  standalone always return true).
  `Queue::resize` now gets physical pixels (`physical_size`: logical size times
  the native pixels per point, rounded, at least 1). Upstream passed the
  logical size, so a Retina window laid out at half size until the next
  `Resized` event. The factor is `native_pixels_per_point`, the one
  egui-baseview divides by for the screen rect; `pixels_per_point` (which
  includes egui's zoom) is the fallback.
  Tests in the same file: `size_reports_the_requested_size_while_the_request_runs`
  and `a_failed_request_restores_the_old_size` fail on the upstream order;
  `no_request_means_no_call`, `the_queue_gets_physical_pixels`. Run them with
  `./guarded-cargo.sh test -p nih_plug_egui --lib` from `plugin/`.
- `src/wrapper/vst3/view.rs` `request_resize`: the `debug_assert_eq!` that
  panicked in debug builds when the host denied `resizeView` becomes a
  `nih_log!` line with the asked size and the result; the function still
  returns false.

Known gap: `vst3/context.rs` `request_resize` returns true whatever the host
answered, and egui-baseview resizes the plug-in's own NSView through
`ViewportCommand::InnerSize` either way. On a VST3 host that refuses, the egui
screen rect still shows the new size while the host's view keeps the old one.
`probes/fl-resize` measures both.
