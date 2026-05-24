# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1] - 2026-05-25

### Fixed

- `ScrollEvent` vertical sign now matches winit: positive = scroll up (toward
  top of doc). Wayland's `wl_pointer.axis` uses the opposite convention
  (positive = scroll down). Previously wayr passed Wayland values through
  unchanged, forcing every consumer to flip the sign on their own to feel
  correct. Now the negation lives at the wayr emission site for `delta`,
  `discrete_steps`, and `high_res_120`. Horizontal axis unchanged (Wayland and
  winit both use positive = scroll right).

[0.2.1]: https://github.com/kryptic-sh/wayr/releases/tag/v0.2.1

## [0.2.0] - 2026-05-24

### Removed (BREAKING)

Scope-creep removal. Wayr's purpose is "winit-equivalent surface area, plus the
`wl_subsurface` embedding that winit can't do." 0.1.10 and 0.1.11 stretched past
that line; neither addition had a winit analogue. Pulled before the API
fossilized.

- `presentation-time` feature (`wp_presentation` binding + always-armed
  `wp_presentation_feedback` chain). Includes:
  - `WindowEvent::FramePresented(PresentationInfo)` and
    `WindowEvent::FrameDiscarded`
  - `Toplevel::last_presented()` and `Toplevel::estimated_next_vblank()`
  - `PresentationInfo`, `PresentFlags`, `PresentationClock` public types
- `Toplevel::set_damage(rect)` and `Toplevel::set_damage_full()` (damage
  tracking pre-commit hooks). Consumers wanting partial-damage compositor blits
  should call `wl_surface.damage_buffer` directly via the `raw-window-handle`
  proxy until a real driver demonstrates the need for a wayr-level API.

### Migration

Apps that used these APIs should:

- Replace `Toplevel::estimated_next_vblank()`-based paint pacing with
  `OutputInfo::refresh_mhz` (unchanged), which is what consumers already had to
  fall back to when the compositor didn't advertise `wp_presentation`.
- Drop `Toplevel::set_damage` / `set_damage_full` calls. The compositor's
  default behaviour (treat the whole surface as damaged on every commit) is what
  0.1.x consumers got before 0.1.11 shipped — going back to that costs a small
  compositor blit but is otherwise transparent.

[0.2.0]: https://github.com/kryptic-sh/wayr/releases/tag/v0.2.0

## [0.1.11] - 2026-05-24

### Added

- `Toplevel::set_damage(rect)` + `Toplevel::set_damage_full()` — queue damage
  regions for the next commit in buffer coordinates. Translates to
  `wl_surface.damage_buffer` immediately; the queued damage takes effect on
  whatever code calls `wl_surface.commit` next (typically wgpu's / vulkano's WSI
  inside `present()` — wayr does not own that commit path). Lets consumers
  report tight dirty rects on partial-frame repaints (chrome, scroll, OSR push
  region) so the compositor can skip blits for unchanged regions. Phase 1 of
  issue #20 — no commit-path ownership required for the damage-tracking win.

[0.1.11]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.11

## [0.1.10] - 2026-05-24

### Added

- `presentation-time` feature flag — opt-in `wp_presentation` binding +
  always-armed `wp_presentation_feedback` chain per `Toplevel`. No commit hook
  required: wayr keeps one feedback object outstanding per surface and re-arms
  in the destructor dispatch. The compositor automatically binds each feedback
  to whichever commit it sees next on the surface (wgpu, vulkano, raw — wayr
  doesn't need to intercept).
  - `WindowEvent::FramePresented(PresentationInfo)` — fired per presented frame.
    Carries hardware-timestamped present time, the compositor's predicted
    refresh period, monotonic frame sequence, sync output, and a `PresentFlags`
    bitfield (vsync / hw_clock / hw_completion / zero_copy).
  - `WindowEvent::FrameDiscarded` — fired when a commit was superseded before
    display (workspace switch mid-flight; rapid re-paint that obsoleted an
    in-flight frame).
  - `Toplevel::last_presented()` — synchronous cache of the latest
    `FramePresented` payload, derived from the event stream. `None` until the
    compositor presents the first frame.
  - `Toplevel::estimated_next_vblank()` — predicted next-vblank `Instant`
    derived from `last_presented`. Feed into `EventLoop::wait_until` to schedule
    the next paint inside the upcoming refresh window for vsync-aligned
    `wgpu.present()`.
  - `PresentationClock` + `PresentFlags` + `PresentationInfo` public types
    re-exported at the crate root.

[0.1.10]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.10

## [0.1.9] - 2026-05-24

### Added

- `WindowEvent::Occluded(bool)` — fired on `xdg_toplevel.state.suspended`
  transitions (xdg-shell v6+). `true` when the compositor has fully obscured the
  surface (minimized / off-workspace / opaque-covered), `false` on reappear.
  Consumers should pause idle repaint while occluded — painting pixels the user
  can't see is wasted CPU / GPU / battery.
- `Toplevel::is_occluded()` — synchronous accessor mirroring the latest
  `Occluded` event value. Useful from `about_to_wait` to decide whether to skip
  a frame.

### Changed

- `xdg_wm_base` is now bound up to v6 (was v5). `wayland-client`'s
  `GlobalList::bind` clamps to the compositor's advertised version, so v5
  sessions keep working unchanged; v6 sessions unlock the new Suspended state
  behind `WindowEvent::Occluded`.

[0.1.9]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.9

## [0.1.8] - 2026-05-24

### Added

- `xdg-activation` feature flag — opt-in `xdg_activation_v1` binding. When the
  compositor advertises the global, consumers gain two new `Toplevel` methods:
  - `Toplevel::request_activation(event_loop)` — kicks off the two-step
    activation handshake (`get_activation_token` → `set_serial(last_input)` →
    `set_surface` → `commit`; on the token's `done` event,
    `activate(token, surface)`). Used by multi-instance apps (e.g. buffr's
    `--new-tab` forwarder) to focus the existing window from a second process.
  - `Toplevel::set_activation_token(event_loop, token)` — direct
    `activate(token, surface)` for the cross-process handoff path where the
    launching process passes a token through the `XDG_ACTIVATION_TOKEN` env var.
    Bypasses the handshake; the token already exists.
- `ActivationError::{Unsupported, NoInputSerial}` returned by the above —
  distinguishes compositor lacks-protocol vs. wayr hasn't-seen-input-yet
  (compositors reject activation without a recent input serial to prevent
  focus-stealing).
- Last input serial (`wl_pointer.button`, `wl_keyboard.key` Pressed,
  `wl_touch.down`) is now tracked on the connection state so
  `xdg_activation_token_v1.set_serial` carries a serial the compositor will
  accept.

[0.1.8]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.8

## [0.1.7] - 2026-05-24

### Added

- `KeyEvent::new_for_test(...)` constructor. `KeyEvent` is `#[non_exhaustive]`
  so consumers couldn't build one with a struct literal — meaning unit tests
  that exercise key-routing logic (modal engines, IME adapters) had to either
  gate the test away or work through a private translation seam. The new
  constructor is a plain pub helper (no `#[cfg(test)]` gate) so it's reachable
  from consumer test modules across the crate boundary.

[0.1.7]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.7

## [0.1.6] - 2026-05-24

### Added

- `EventLoop::wait_until(deadline)` — single-shot cap on the next
  `blocking_pump` sleep. Lets consumers drive animation pacing from
  `about_to_wait` (call it with the next-frame deadline) so the loop wakes on
  time instead of waiting the default 50 ms idle cap. Composes with internal
  deadlines (key-repeat, blocking_pump max) via min. Real input still preempts
  via `poll(2)`.
- `OutputInfo::refresh_mhz` — refresh rate of the active mode, in millihertz
  (e.g. `60000` = 60 Hz, `144000` = 144 Hz). Wired from `wl_output.mode` events
  flagged `current`; multi-mode displays no longer have their refresh
  overwritten by stale advertised modes. `0` until the compositor sends the
  first mode event.

[0.1.6]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.6

## [0.1.5] - 2026-05-23

### Added

- Key-repeat is now driven by wayr. Holding a key fires a stream of
  `WindowEvent::Key { repeat: true, .. }` events paced by the compositor's
  `wl_keyboard.repeat_info` (delay + rate). xkbcommon's
  `keymap.key_repeats(...)` is consulted so modifier keys and similar
  non-repeatable keys don't generate repeats. The first repeat fires after
  `delay_ms` ms; subsequent repeats every `1000 / rate_hz` ms. Releasing the
  repeating key, focus loss, or the compositor sending `rate == 0` (repeat
  disabled) all stop the synthesis cleanly.
- `EventLoop`'s `blocking_pump` timeout is now capped at the next repeat-fire
  deadline (or 50 ms, whichever is sooner), so repeats fire on time even when no
  other events arrive.

[0.1.5]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.5

## [0.1.4] - 2026-05-23

### Changed

- `Toplevel` and `LayerSurface` are now `Send + Sync + 'static`. Internal shared
  state moved from `Rc<RefCell<_>>` to `Arc<Mutex<_>>` (no contention in
  practice — wayr dispatch runs single-threaded; the mutex is
  cheap-uncontended), and each surface now holds its own cheap-clone of the
  wayland-client `Connection` to keep the socket alive across `Drop` and to
  satisfy [`HasDisplayHandle`].

### Added

- `HasDisplayHandle` impl on `Toplevel` and `LayerSurface`. Combined with the
  existing `HasWindowHandle` impl and the new `Send + Sync + 'static` guarantee,
  `Arc<Toplevel>` and `Arc<LayerSurface>` now satisfy wgpu's
  `SurfaceTarget::Window` bound — consumers can call
  `instance.create_surface(arc_toplevel)` on the safe path instead of
  `create_surface_unsafe` with raw pointers. wgpu Surface holds an Arc keeping
  the source alive across its own drop, so Wayland teardown ordering is correct
  (no late-drop `wl_surface.destroy()` racing with wgpu's Vulkan cleanup — a
  buffr shutdown SIGSEGV diagnosed against 0.1.3).
- Compile-time assertions on `Toplevel` + `LayerSurface` that they satisfy
  `Send + Sync + 'static`.

[0.1.4]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.4

## [0.1.3] - 2026-05-23

### Fixed

- `Resized` and `ScaleFactorChanged` are now deduplicated against the
  last-emitted value. Compositors reconfigure surfaces for many reasons that
  don't change the size or scale — activated-bit flip on focus change,
  decoration update, tiled-state shuffle — and unconditionally re-emitting
  `Resized(new_size)` for every configure ack caused heavy consumers (e.g. CEF
  host resize-cascade in buffr) to thrash on every Alt-Tab. wayr now tracks
  `last_emitted_size` + `last_emitted_scale` per surface and only emits the
  event when the value actually moved. `RedrawRequested` still fires on every
  configure ack — the compositor expects a fresh frame regardless of size delta.

[0.1.3]: https://github.com/kryptic-sh/wayr/releases/tag/v0.1.3

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

[Unreleased]: https://github.com/kryptic-sh/wayr/compare/v0.2.0...HEAD
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
