# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-05-23

### Fixed

- `KeyEvent::text` no longer carries ASCII control characters (`"\r"` for
  Return, `"\u{8}"` for BackSpace, `"\t"` for Tab, `"\u{1b}"` for Escape,
  `"\u{7f}"` for Delete, …). xkbcommon emits these alongside the keysym name,
  but consumers consistently want a clean "text => printable character; key_code
  => everything else" dispatch — the previous behaviour forced every consumer to
  add an `is_ascii_control` filter before reading `text`. wayr now applies the
  filter at the source. Matches winit's `KeyEvent::text` semantics.

### Changed

- `KeyEvent::text` docstring updated to spell out the control-character
  exclusion and that `Some("")` never occurs.

[0.1.2]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.2

## [0.1.1] - 2026-05-23

### Fixed

- `Surface::request_redraw` was a no-op in v0.1.0. The doc-stub comment promised
  a synthetic `RedrawRequested` would arrive, but the implementation didn't
  queue one — consumers relying on `request_redraw` to drive paints (buffr's
  chrome flow) saw frozen UI between configures. Fixed with a `needs_redraw`
  flag on shared surface state, drained by the run loop into one
  `WindowEvent::RedrawRequested` per surface per iteration (matching winit's
  coalescing semantics). Real `wl_surface.frame()` compositor-paced redraws stay
  queued for a future release; this immediate path is sufficient for the
  consumer that needed it.

[Unreleased]: https://github.com/kryptic-sh/wayr/compare/v0.1.2...HEAD
[0.1.1]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.1

## [0.1.0] - 2026-05-23

First public release. Wayland-first windowing toolkit for Rust; Linux-only by
design.

### Added

- **Connection layer** — `wl_compositor` / `wl_subcompositor` / `wl_shm` /
  `wl_seat` / `xdg_wm_base` binding via registry roundtrip in `connect_to_env`.
  `wl_seat` v7. Optional globals bind only when the matching feature is on.
- **Toplevel windows** — `Toplevel` + `ToplevelBuilder` over `xdg_surface` +
  `xdg_toplevel`. `with_title` / `with_app_id` / `with_initial_size` /
  `with_min_size` / `with_max_size`. Configure handshake auto-acks; `Resized` +
  `ScaleFactorChanged` + `RedrawRequested` events emitted on every cycle.
- **Event loop** — `EventLoop::run_app` (blocking) + `EventLoop::poll`
  (non-blocking) over a `prepare_read` + `poll(2)` pump.
  `EventLoopProxy::send_event` is `Send + Sync` via `mpsc`. `ApplicationHandler`
  trait mirrors winit's shape.
- **raw-window-handle 0.6** — `HasWindowHandle` on `Toplevel`, `LayerSurface`,
  `Subsurface`; `HasDisplayHandle` on `EventLoop`. wgpu / vulkano / glow
  consumers plug in directly.
- **Pointer** — `wl_pointer.enter` / `.leave` / `.motion` / `.button` / `.axis`
  / `.axis_discrete` / `.axis_value120` / `.frame`. Modifier-aware button
  events; smooth + discrete + 1/120 high-res scroll surfaced in a single
  `ScrollEvent`.
- **Keyboard (`#[cfg(feature = "<core>")]`)** — `wl_keyboard` with xkbcommon
  keymap parsing from the compositor-mmap'd fd. `KeyCode` + modifier state;
  `Modifiers` from `wl_keyboard.modifiers`.
- **Layer-shell** (`layer-shell` feature) — `zwlr_layer_shell_v1` v4.
  `LayerSurface` with `set_anchor` / `set_exclusive_zone` / `set_margin` /
  `set_keyboard_interactivity` / `set_size`. Configure and close events.
- **Subsurface** (`subsurface` feature) — `wl_subsurface` child of a parent
  `Toplevel`. Lifetime-free runtime invariant (drop child before parent).
  `set_position` / `set_geometry` / `set_sync` / `set_desync`. Raw `wl_surface*`
  ptr accessor for FFI embedders (WPE WebKit etc.).
- **Text-input v3 IME** (`text-input` feature) — `zwp_text_input_manager_v3` +
  per-seat `zwp_text_input_v3`. `Ime` accessor with `enable` / `disable` /
  `set_cursor_rect` / `set_purpose` / `set_hint` / `set_content_type` /
  `set_surrounding_text`.
  `WindowEvent::Ime(Preedit / Commit / DeleteSurroundingText)` events.
- **Cursor-shape** (`cursor-shape` feature) — `wp_cursor_shape_manager_v1` v2.
  Full W3C cursor enum (35 variants).
  `Toplevel::set_cursor(&EventLoop, CursorIcon)` / `LayerSurface::set_cursor`
  (cursor is per-seat in Wayland, not per-surface).
- **Touch** — `wl_touch` dispatch;
  `WindowEvent::Touch(TouchEvent { id, phase, position })` with `Started` /
  `Moved` / `Ended` / `Cancelled` phases.
- **Fractional scaling + multi-output** (`fractional-scale` feature) —
  `wl_output` per-monitor state (scale, geometry, mode, name, description)
  snapshotted via `EventLoop::outputs()` → `Vec<OutputInfo>`. `wl_surface.enter`
  / `.leave` drives per-surface `touched_outputs`; integer fallback scale = max
  output scale. `wp_fractional_scale_manager_v1` + `wp_viewporter` auto-applied;
  `Toplevel::physical_size()` for buffer-size planning;
  `WindowEvent::ScaleFactorChanged` carries the fractional value.
- **Headless e2e test harness** — `tests/run-e2e.sh` spawns a private sway with
  `WLR_BACKENDS=headless` and runs every `#[ignore]`'d integration test. Six
  suites: toplevel handshake, layer-shell handshake, subsurface attach,
  cursor-shape, IME, output enumeration. Wired into CI.
- **Example** — `examples/toplevel.rs` paints a wgpu 29 clear-to-colour, proves
  the raw-window-handle 0.6 integration end-to-end.

### Out of scope

- Clipboard / data-device — use
  [`hjkl-clipboard`](https://crates.io/crates/hjkl-clipboard) instead.
- Anything non-Wayland. No X11, macOS, Windows, mobile, or web.

[0.1.0]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.0
