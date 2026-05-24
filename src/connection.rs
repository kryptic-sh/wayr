//! Wayland connection + registry roundtrip + global binding.
//!
//! Internal module; the public API surface is constructed via
//! [`crate::EventLoop::new`].
//!
//! ## Why bind manually instead of using `smithay-client-toolkit`?
//!
//! sctk is a fine library, but it ships its own opinions about
//! window lifecycles + dispatch macros that conflict with wayr's
//! taxonomy (three explicit surface types, lifetime-bound subsurfaces).
//! The handful of globals we need is small enough to bind directly
//! without inheriting sctk's design assumptions.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use tracing::{debug, warn};
use wayland_client::globals::{Global, GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_keyboard::{
    Event as WlKeyboardEvent, KeyState as WlKeyState, KeymapFormat, WlKeyboard,
};
use wayland_client::protocol::wl_output::{Event as WlOutputEvent, Subpixel, Transform, WlOutput};
use wayland_client::protocol::wl_pointer::{
    self, ButtonState as WlButtonState, Event as WlPointerEvent, WlPointer,
};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{Capability, Event as WlSeatEvent, WlSeat};
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
use wayland_client::protocol::wl_subsurface::WlSubsurface;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::protocol::wl_touch::{Event as WlTouchEvent, WlTouch};
use wayland_client::{Connection as WlConnection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::xdg::shell::client::xdg_surface::{Event as XdgSurfaceEvent, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{
    Event as XdgToplevelEvent, State as XdgToplevelStateFlag, XdgToplevel,
};
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;

use crate::error::{Error, Result};
use crate::event::{Event, WindowEvent};
use crate::geometry::{Position, Size};
use crate::keyboard::{KeyCode, KeyEvent, KeyState as WayrKeyState, Modifiers, ScanCode};
use crate::output::{OutputId, OutputInfo};
use crate::pointer::{
    AxisDirection, AxisSource, PointerButton, PointerButtonState, PointerPosition, ScrollEvent,
};
use crate::surface::SurfaceId;
use crate::touch::{TouchEvent, TouchId, TouchPhase};

#[cfg(feature = "text-input")]
use crate::ime::ImeEvent;
#[cfg(feature = "text-input")]
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3;
#[cfg(feature = "text-input")]
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    Event as TextInputV3Event, ZwpTextInputV3,
};

#[cfg(feature = "cursor-shape")]
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;
#[cfg(feature = "cursor-shape")]
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;

#[cfg(feature = "fractional-scale")]
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
#[cfg(feature = "fractional-scale")]
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
    Event as WpFractionalScaleEvent, WpFractionalScaleV1,
};
#[cfg(feature = "fractional-scale")]
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
#[cfg(feature = "fractional-scale")]
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

#[cfg(feature = "xdg-activation")]
use wayland_protocols::xdg::activation::v1::client::xdg_activation_token_v1::{
    Event as XdgActivationTokenEvent, XdgActivationTokenV1,
};
#[cfg(feature = "xdg-activation")]
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

/// Owning handle for the live Wayland connection + bound globals.
///
/// One per [`crate::EventLoop`]. Dropped on event-loop teardown; the
/// underlying socket closes when the last `WlConnection` Arc drops.
pub(crate) struct Connection {
    /// The raw wayland-client connection. Re-exposed to consumers via
    /// `EventLoop::raw_display_handle()` so wgpu can construct a
    /// surface from it.
    pub(crate) wl: WlConnection,

    /// Default event queue. `wayr` runs everything on this queue —
    /// the calloop adapter dispatches it from the event loop's pump.
    /// `EventQueue::handle()` is cloneable, so the per-surface code
    /// stores its own `QueueHandle<State>` rather than borrowing this
    /// field.
    pub(crate) queue: EventQueue<State>,

    /// All bound protocol globals.
    pub(crate) globals: Globals,
}

/// Bag of bound globals. Required ones are non-`Option`; optional
/// (feature-gated) globals are `Option`.
pub(crate) struct Globals {
    /// `wl_compositor` — creates `wl_surface`s. Always required.
    pub(crate) compositor: WlCompositor,

    /// `wl_subcompositor` — creates `wl_subsurface`s. Always bound;
    /// surface code behind the `subsurface` feature uses it. We bind
    /// unconditionally because the cost is one proxy + ~64 bytes, and
    /// we want consumer code that conditionally compiles subsurface
    /// support to get a useful error if the compositor missed
    /// advertising it.
    pub(crate) subcompositor: WlSubcompositor,

    /// `wl_shm` — shared memory for software-rendered surfaces +
    /// cursor themes. Always required (cursor fallback path).
    pub(crate) shm: WlShm,

    /// `wl_seat` — pointer/keyboard/touch input. v0.1 binds a single
    /// seat (`wl_seat@1`); multi-seat support is post-MVP.
    pub(crate) seat: WlSeat,

    /// `xdg_wm_base` — top-level windows. Always required (pings).
    pub(crate) xdg_wm_base: XdgWmBase,

    /// All `wl_output`s the compositor advertised. Per-output state
    /// (scale, geometry, mode, name) is mirrored into [`State::outputs`]
    /// by the dispatch handler; this Vec just keeps the proxies alive.
    pub(crate) outputs: Vec<WlOutput>,

    /// `zwlr_layer_shell_v1` — only when the `layer-shell` feature is
    /// on AND the compositor advertises it.
    #[cfg(feature = "layer-shell")]
    pub(crate) layer_shell: Option<
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
    >,

    /// `zwp_text_input_manager_v3` — only when the `text-input`
    /// feature is on AND the compositor advertises it.
    #[cfg(feature = "text-input")]
    pub(crate) text_input_manager: Option<ZwpTextInputManagerV3>,

    /// `wp_cursor_shape_manager_v1` — only when the `cursor-shape`
    /// feature is on AND the compositor advertises it. Toplevel /
    /// LayerSurface fall back to a no-op set_cursor when missing.
    #[cfg(feature = "cursor-shape")]
    pub(crate) cursor_shape_manager: Option<WpCursorShapeManagerV1>,

    /// `wp_fractional_scale_manager_v1` + `wp_viewporter` — `None` when
    /// the `fractional-scale` feature is off or the compositor lacks
    /// the manager. Per-surface objects are spawned at build time.
    #[cfg(feature = "fractional-scale")]
    pub(crate) fractional_scale_manager: Option<WpFractionalScaleManagerV1>,
    /// `wp_viewporter` — bound alongside fractional-scale, used to
    /// `set_destination` the surface to logical pixels while the
    /// consumer renders at physical resolution.
    #[cfg(feature = "fractional-scale")]
    pub(crate) viewporter: Option<WpViewporter>,

    /// `xdg_activation_v1` — present when the `xdg-activation` feature
    /// is on AND the compositor advertises the global. Drives
    /// [`crate::Toplevel::request_activation`].
    #[cfg(feature = "xdg-activation")]
    pub(crate) xdg_activation: Option<XdgActivationV1>,
}

/// Per-output mirrored state. Populated from the wl_output dispatch
/// handler so callers (and the per-surface scale resolver) can read
/// it without driving wayland round-trips themselves.
#[derive(Debug, Default, Clone)]
pub(crate) struct OutputState {
    pub(crate) id: OutputId,
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) scale: i32,
    pub(crate) physical_size: Size,
    pub(crate) position: (i32, i32),
    /// Refresh rate in mHz from the most recent `wl_output.mode`
    /// event flagged `current`. `0` until the compositor sends mode.
    pub(crate) refresh_mhz: i32,
    /// Pending state staged via `wl_output.geometry` / `.mode` / `.scale`
    /// is only applied on `done` (per the protocol's atomic-set
    /// guarantee). We mirror straight into the live fields above and
    /// expose `OutputInfo` snapshots on each `done`.
    pub(crate) ready: bool,
}

impl OutputState {
    pub(crate) fn snapshot(&self) -> OutputInfo {
        OutputInfo {
            id: self.id,
            name: self.name.clone(),
            description: self.description.clone(),
            scale: self.scale.max(1),
            physical_size: self.physical_size,
            position: self.position,
            refresh_mhz: self.refresh_mhz,
        }
    }
}

/// Per-layer-surface state, parallel to [`ToplevelState`]. Shared
/// between the [`crate::LayerSurface`] public API and the
/// `ZwlrLayerSurfaceV1` dispatch handler.
#[cfg(feature = "layer-shell")]
#[derive(Debug, Default)]
pub(crate) struct LayerSurfaceState {
    /// Current logical size last committed by configure ack.
    pub(crate) current_size: Size,
    /// Preferred size from the builder. Used when the compositor's
    /// configure returns `0` on an axis ("you pick").
    pub(crate) preferred_size: Size,
    /// Effective scale factor — composed from (`fractional_scale_120 /
    /// 120` if present) or `max(touched_outputs.scale)` otherwise.
    pub(crate) scale_factor: f64,
    /// Size last surfaced via `Resized` (dedupe; see
    /// [`ToplevelState::last_emitted_size`]).
    pub(crate) last_emitted_size: Size,
    /// Scale last surfaced via `ScaleFactorChanged` (dedupe; see
    /// [`ToplevelState::last_emitted_scale`]).
    pub(crate) last_emitted_scale: f64,
    /// Consumer called `LayerSurface::request_redraw()` and hasn't been
    /// served yet. See [`ToplevelState::needs_redraw`].
    pub(crate) needs_redraw: bool,
    /// Last `preferred_scale` from `wp_fractional_scale_v1` (in 1/120
    /// units). `None` until the compositor sends one OR the
    /// `fractional-scale` feature is off.
    pub(crate) fractional_scale_120: Option<u32>,
    /// Outputs this surface currently overlaps (driven by
    /// `wl_surface.enter` / `.leave`). Empty until the compositor
    /// reports the first `enter`.
    pub(crate) touched_outputs: HashSet<OutputId>,
    /// Closed by compositor.
    pub(crate) closed: bool,
    /// Clone of the surface's `wp_viewport` proxy. See
    /// [`ToplevelState::viewport`].
    #[cfg(feature = "fractional-scale")]
    pub(crate) viewport: Option<wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport>,
}

/// Per-toplevel state mutated by dispatch handlers and observed by
/// the [`crate::Toplevel`] public methods. Shared between the two via
/// an `Arc<Mutex<_>>`.
#[derive(Debug, Default)]
pub(crate) struct ToplevelState {
    /// Current logical size as last committed by configure ack. Zero
    /// until the first configure resolves.
    pub(crate) current_size: Size,
    /// Initial size requested at build time (used until the first
    /// non-zero configure arrives).
    pub(crate) preferred_size: Size,
    /// Latest configure serial pending an ack on the next commit.
    /// `None` once acked.
    pub(crate) pending_ack: Option<u32>,
    /// Whether the toplevel was destroyed (close-requested + acted on
    /// by consumer, OR compositor-side destroy).
    pub(crate) closed: bool,
    /// Activated / focused / fullscreen / maximised state from the
    /// last configure. v0.1 only surfaces `Focused` / `Unfocused`
    /// (activated bit).
    pub(crate) activated: bool,
    /// `xdg_toplevel.state.suspended` from the last configure
    /// (xdg-shell v6+). True when the compositor has fully obscured
    /// the surface (minimized, occluded by an opaque window, other
    /// workspace). Drives `WindowEvent::Occluded(bool)` transitions
    /// and is exposed via `Toplevel::is_occluded()`. Stays `false` on
    /// v5 compositors that never advertise it.
    pub(crate) suspended: bool,
    /// Effective scale factor (composed: fractional if available,
    /// otherwise `max(touched_outputs.scale)`).
    pub(crate) scale_factor: f64,
    /// Size last surfaced to the consumer via a `Resized` event.
    /// Used to dedupe spurious configures: the compositor reconfigures
    /// the surface for many reasons that don't change the size
    /// (activated-bit flip on focus change, decoration update, …),
    /// and re-emitting `Resized` with an unchanged value causes heavy
    /// consumers (CEF host resize cascade, wgpu surface.configure)
    /// to do unnecessary work.
    pub(crate) last_emitted_size: Size,
    /// Scale last surfaced to the consumer via `ScaleFactorChanged`.
    /// Same dedupe rationale as [`Self::last_emitted_size`].
    pub(crate) last_emitted_scale: f64,
    /// Consumer called `Toplevel::request_redraw()` and hasn't been
    /// served yet. The run loop drains this into a synthetic
    /// `WindowEvent::RedrawRequested` once per iteration (coalescing
    /// repeat requests within a tick), matching winit's semantics.
    pub(crate) needs_redraw: bool,
    /// Last `preferred_scale` from `wp_fractional_scale_v1` (in 1/120
    /// units). `None` until the compositor sends one.
    pub(crate) fractional_scale_120: Option<u32>,
    /// Outputs this surface currently overlaps (driven by
    /// `wl_surface.enter` / `.leave`). Keys are `OutputId.0` for cheap
    /// copyable lookups; the per-output state lives in [`State::outputs`].
    pub(crate) touched_outputs: HashSet<OutputId>,
    /// Clone of the surface's `wp_viewport` proxy. Held in state so
    /// the configure dispatch handler can auto-apply
    /// `set_destination(logical_w, logical_h)` whenever the size
    /// changes — keeping the consumer's rendered (physical-sized)
    /// buffer correctly downscaled to the logical surface bounds.
    #[cfg(feature = "fractional-scale")]
    pub(crate) viewport: Option<wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport>,
}

/// Per-seat text-input state. v0.1 supports a single seat.
///
/// `zwp_text_input_v3` is a single object per seat that follows
/// keyboard focus (via its own `enter`/`leave` events). Each
/// `Toplevel` / `LayerSurface` gets a [`crate::Ime`] accessor that
/// shares this state via `Arc<Mutex<_>>`.
#[cfg(feature = "text-input")]
#[derive(Default)]
pub(crate) struct TextInputState {
    /// The per-seat `zwp_text_input_v3` proxy. `None` until the
    /// compositor advertises the manager AND wl_seat is bound.
    pub(crate) wp: Option<ZwpTextInputV3>,
    /// Whether the consumer last requested `enable` on this seat.
    /// Mirrors the protocol's "is enabled" state for the surface
    /// owning focus.
    pub(crate) enabled: bool,
    /// Last `serial` from the compositor's `done` event — required
    /// when committing client state.
    pub(crate) last_done_serial: u32,
    /// Surface the text_input has `entered` (= got keyboard focus
    /// for IME purposes). `None` between enter/leave pairs.
    pub(crate) focused_surface: Option<SurfaceId>,
    /// Pending events accumulated since the last `done` boundary —
    /// drained into `WindowEvent::Ime` on `done`.
    pub(crate) pending_preedit: Option<(String, Option<u32>)>,
    pub(crate) pending_commit: Option<String>,
    pub(crate) pending_delete: Option<(u32, u32)>,
}

/// Per-keyboard state. v0.1 supports a single seat keyboard.
#[derive(Default)]
pub(crate) struct KeyboardState {
    /// The `wl_keyboard` proxy when the seat advertised keyboard
    /// capability.
    pub(crate) wl_keyboard: Option<WlKeyboard>,
    /// Loaded xkbcommon state. `None` until the compositor sends the
    /// initial `wl_keyboard.keymap` event.
    pub(crate) xkb: Option<XkbState>,
    /// Surface that currently has keyboard focus. `None` between
    /// enter/leave pairs.
    pub(crate) focused_surface: Option<SurfaceId>,
    /// Cached modifier state — updated on `wl_keyboard.modifiers`,
    /// surfaced on every `KeyEvent` + `PointerButton`.
    pub(crate) modifiers: Modifiers,
    /// Repeat rate in keys per second from the compositor's
    /// `wl_keyboard.repeat_info` event. `0` disables repeat
    /// (compositor explicitly said no). Default `30` if the
    /// compositor never advertises (legacy behaviour).
    pub(crate) repeat_rate_hz: i32,
    /// Repeat delay in milliseconds before the first repeat fires
    /// after a key is held. `0` disables. Default `500ms`.
    pub(crate) repeat_delay_ms: i32,
    /// The currently-held repeatable key, if any. Set on Key Pressed
    /// (when the keymap says the key repeats); cleared on Released
    /// (when scancode matches) or on focus loss.
    pub(crate) repeating: Option<RepeatingKey>,
}

/// State for synthesizing key-repeat events. Holds enough to
/// reconstruct a `KeyEvent` clone for the repeat dispatch, plus the
/// next-fire deadline.
#[derive(Clone)]
pub(crate) struct RepeatingKey {
    pub(crate) surface_id: SurfaceId,
    pub(crate) scancode: ScanCode,
    pub(crate) key_code: KeyCode,
    pub(crate) text: Option<String>,
    pub(crate) next_fire_at: std::time::Instant,
}

/// xkbcommon `Context` + `Keymap` + `State`, bundled. Constructed
/// from the `wl_keyboard.keymap` event.
pub(crate) struct XkbState {
    pub(crate) _context: xkbcommon::xkb::Context,
    pub(crate) keymap: xkbcommon::xkb::Keymap,
    pub(crate) state: xkbcommon::xkb::State,
}

/// Per-pointer focus + accumulator state. v0.1 supports a single seat
/// pointer; multi-seat / multi-pointer is post-MVP.
#[derive(Default)]
pub(crate) struct PointerState {
    /// The `wl_pointer` proxy, when the seat advertised pointer
    /// capability. `None` until capabilities arrive (or if the seat
    /// has no pointer).
    pub(crate) wl_pointer: Option<WlPointer>,
    /// Surface the pointer is currently over. `None` between
    /// enter/leave pairs.
    pub(crate) focused_surface: Option<SurfaceId>,
    /// Latest `wl_pointer.enter` serial. Required when calling
    /// `wp_cursor_shape_device_v1.set_shape(serial, …)` — the
    /// compositor ignores the request unless the serial matches.
    pub(crate) enter_serial: u32,
    /// Accumulated axis state since the last wl_pointer.frame. v0.1
    /// flushes on every `frame` event; finer batching is post-MVP.
    pub(crate) axis_vertical: f64,
    pub(crate) axis_horizontal: f64,
    pub(crate) axis_discrete_v: i32,
    pub(crate) axis_discrete_h: i32,
    pub(crate) axis_value120_v: i32,
    pub(crate) axis_value120_h: i32,
    pub(crate) axis_source: Option<AxisSource>,
    pub(crate) axis_pending: bool,
    /// Cursor-shape device for this pointer. Eagerly created in the
    /// `WlSeat::Capabilities` dispatch when both the pointer cap is
    /// present and the cursor-shape manager global was bound.
    #[cfg(feature = "cursor-shape")]
    pub(crate) cursor_shape_device: Option<WpCursorShapeDeviceV1>,
}

/// Per-touch state. v0.1 supports a single seat touch; multi-seat /
/// multi-touchscreen is post-MVP.
#[derive(Default)]
pub(crate) struct TouchState {
    /// `wl_touch` proxy if the seat advertised the touch capability.
    pub(crate) wl_touch: Option<WlTouch>,
    /// Last known position per active contact, so [`TouchPhase::Ended`]
    /// / [`TouchPhase::Cancelled`] events can carry the contact's
    /// final position (the protocol's `up` event has no coords).
    pub(crate) positions: HashMap<i32, Position>,
    /// Which surface every active contact entered on. Cleared on `up`
    /// / `cancel` so an entry only exists between matching down / up.
    pub(crate) surfaces: HashMap<i32, SurfaceId>,
}

/// Dispatch state. Threaded through every wayland-client `Dispatch`
/// impl in this crate.
#[derive(Default)]
pub(crate) struct State {
    /// Pending events drained by [`crate::EventLoop::run_app`] /
    /// [`crate::EventLoop::poll`] each iteration. v0.1 only carries
    /// `Event<()>`; the generic `T` parameter on the public
    /// `EventLoop<T>` is bridged by serialising user events through
    /// the proxy channel.
    pub(crate) pending_events: Vec<Event<()>>,

    /// Per-toplevel state. `SurfaceId` is the lookup key — the
    /// `XdgSurface` and `XdgToplevel` proxies are bound with the
    /// `SurfaceId` as their user-data so dispatch handlers can find
    /// the matching `Arc<Mutex<ToplevelState>>`.
    pub(crate) toplevels: HashMap<SurfaceId, Arc<Mutex<ToplevelState>>>,

    /// Per-layer-surface state.
    #[cfg(feature = "layer-shell")]
    pub(crate) layer_surfaces: HashMap<SurfaceId, Arc<Mutex<LayerSurfaceState>>>,

    /// Lookup from `wl_surface` to `SurfaceId`. Pointer / keyboard
    /// dispatch handlers receive a `&WlSurface` and need the matching
    /// `SurfaceId` to route the event.
    pub(crate) surface_id_by_wl: HashMap<WlSurface, SurfaceId>,

    /// Pointer state.
    pub(crate) pointer: PointerState,

    /// Keyboard state.
    pub(crate) keyboard: KeyboardState,

    /// Touch state.
    pub(crate) touch: TouchState,

    /// Cursor-shape manager proxy clone — held in state so the
    /// WlSeat::Capabilities dispatch can lazy-create a
    /// `wp_cursor_shape_device_v1` once the pointer arrives. `None`
    /// when the compositor doesn't advertise the global.
    #[cfg(feature = "cursor-shape")]
    pub(crate) cursor_shape_manager: Option<WpCursorShapeManagerV1>,

    /// Text-input (IME) state. v0.1 supports a single seat.
    #[cfg(feature = "text-input")]
    pub(crate) text_input: TextInputState,

    /// Monotonic `SurfaceId` counter. Wraps at u64::MAX (effectively
    /// never).
    pub(crate) next_surface_id: u64,

    /// Monotonic `OutputId` counter. Allocated lazily when an
    /// `OutputState` entry is first inserted.
    pub(crate) next_output_id: u64,

    /// Per-output state, keyed by the bound `wl_output` proxy.
    /// Populated by the wl_output dispatch handler (the registry
    /// roundtrip in `connect_to_env` binds the proxies but the
    /// geometry/mode/scale/name events arrive asynchronously).
    pub(crate) outputs: HashMap<WlOutput, OutputState>,

    /// Set by `EventLoop::exit`. Drives the run loop to bail.
    pub(crate) exit_requested: bool,

    /// Optional single-shot deadline set by `EventLoop::wait_until`.
    /// The run loop caps `blocking_pump`'s timeout at this value
    /// (taking the minimum against other internal deadlines), then
    /// clears it. Consumers re-arm each iteration from
    /// `about_to_wait`.
    pub(crate) wait_until: Option<std::time::Instant>,

    /// Most recent input-event serial — last `wl_pointer.button`,
    /// `wl_keyboard.key` (press only), or `wl_touch.down` — used by
    /// [`crate::Toplevel::request_activation`] for
    /// `xdg_activation_token_v1.set_serial`. Compositors validate that
    /// activation requests carry a recent input serial to prevent
    /// background processes stealing focus. `0` until the user
    /// interacts with any of our surfaces (matches the wayland
    /// "no serial yet" convention).
    pub(crate) last_input_serial: u32,

    /// Pending `xdg_activation_token_v1` requests awaiting the `done`
    /// event. Keyed by the token proxy; the stored `WlSurface` is the
    /// surface to activate once the token string arrives. Entries are
    /// removed in the token's `done` dispatch arm before
    /// `xdg_activation_v1.activate` fires.
    #[cfg(feature = "xdg-activation")]
    pub(crate) pending_activation_tokens: HashMap<XdgActivationTokenV1, WlSurface>,

    /// Clone of the `xdg_activation_v1` manager proxy (when bound) so
    /// the token-`done` dispatch arm can call
    /// `activate(token, surface)` without re-walking the proxy graph.
    /// `None` when the feature is on but the compositor doesn't
    /// advertise the global.
    #[cfg(feature = "xdg-activation")]
    pub(crate) xdg_activation_manager: Option<XdgActivationV1>,
}

impl State {
    /// Allocate a fresh `SurfaceId`.
    pub(crate) fn alloc_surface_id(&mut self) -> SurfaceId {
        self.next_surface_id = self.next_surface_id.wrapping_add(1);
        SurfaceId::from_raw(self.next_surface_id)
            .expect("next_surface_id never zero after wrapping_add")
    }

    /// Allocate a fresh `OutputId`.
    pub(crate) fn alloc_output_id(&mut self) -> OutputId {
        self.next_output_id = self.next_output_id.wrapping_add(1);
        OutputId(self.next_output_id)
    }

    /// Recompute the effective scale_factor for a toplevel using the
    /// composition rule (fractional if set, otherwise the max integer
    /// scale of touched outputs, defaulting to 1.0). Returns the new
    /// value so the caller can decide whether to emit
    /// `ScaleFactorChanged`.
    pub(crate) fn resolved_scale_for_toplevel(&self, st: &ToplevelState) -> f64 {
        if let Some(s120) = st.fractional_scale_120 {
            return s120 as f64 / 120.0;
        }
        st.touched_outputs
            .iter()
            .filter_map(|oid| {
                self.outputs
                    .values()
                    .find(|o| o.id == *oid)
                    .map(|o| o.scale.max(1))
            })
            .max()
            .unwrap_or(1) as f64
    }

    #[cfg(feature = "layer-shell")]
    pub(crate) fn resolved_scale_for_layer(&self, st: &LayerSurfaceState) -> f64 {
        if let Some(s120) = st.fractional_scale_120 {
            return s120 as f64 / 120.0;
        }
        st.touched_outputs
            .iter()
            .filter_map(|oid| {
                self.outputs
                    .values()
                    .find(|o| o.id == *oid)
                    .map(|o| o.scale.max(1))
            })
            .max()
            .unwrap_or(1) as f64
    }
}

impl Connection {
    /// Connect to `WAYLAND_DISPLAY`, perform the registry roundtrip,
    /// and bind every global wayr needs.
    pub(crate) fn connect_to_env() -> Result<Self> {
        let wl = WlConnection::connect_to_env().map_err(|err| {
            Error::NotWayland(format!(
                "wayland-client connection failed: {err} \
                 (is WAYLAND_DISPLAY set?)"
            ))
        })?;

        // `registry_queue_init` does the synchronous roundtrip on a
        // private queue and returns the global list — exactly the
        // pattern that was hard for buffr against winit's queue, but
        // easy here because wayr OWNS the connection.
        let (global_list, queue) = registry_queue_init::<State>(&wl).map_err(|err| {
            Error::Io(std::io::Error::other(format!(
                "registry queue init failed: {err}"
            )))
        })?;

        let qh = queue.handle();

        // Bind required globals via the global-list helper. Each call
        // returns the highest version we support that the compositor
        // advertised, or errors if the global is missing entirely.
        let compositor: WlCompositor = bind_required(&global_list, &qh, "wl_compositor", 5)?;
        let subcompositor: WlSubcompositor =
            bind_required(&global_list, &qh, "wl_subcompositor", 1)?;
        let shm: WlShm = bind_required(&global_list, &qh, "wl_shm", 1)?;
        let seat: WlSeat = bind_required(&global_list, &qh, "wl_seat", 7)?;
        // v6 adds the `suspended` state — surfaced as
        // `WindowEvent::Occluded(true)` so consumers can pause idle
        // repaint when fully obscured. `bind` clamps to whatever the
        // compositor advertises, so v5 sessions get v5 silently and
        // Suspended just never fires.
        let xdg_wm_base: XdgWmBase = bind_required(&global_list, &qh, "xdg_wm_base", 6)?;

        // Multi-output: bind all advertised wl_output globals. Each
        // surface tracks which outputs it touches via wl_surface.enter
        // / leave in Phase 4.
        let outputs: Vec<WlOutput> = global_list.contents().with_list(|globals| {
            globals
                .iter()
                .filter(|g: &&Global| g.interface == "wl_output")
                .map(|g: &Global| {
                    // wl_output is version 4 since Wayland 1.21;
                    // bind highest supported.
                    let version = g.version.min(4);
                    global_list
                        .registry()
                        .bind::<WlOutput, (), State>(g.name, version, &qh, ())
                })
                .collect()
        });
        debug!(count = outputs.len(), "bound wl_output globals");

        #[cfg(feature = "layer-shell")]
        let layer_shell: Option<
            wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
        > = bind_optional(&global_list, &qh, "zwlr_layer_shell_v1", 4);

        #[cfg(feature = "text-input")]
        let text_input_manager: Option<ZwpTextInputManagerV3> =
            bind_optional(&global_list, &qh, "zwp_text_input_manager_v3", 1);

        #[cfg(feature = "cursor-shape")]
        let cursor_shape_manager: Option<WpCursorShapeManagerV1> =
            bind_optional(&global_list, &qh, "wp_cursor_shape_manager_v1", 2);

        #[cfg(feature = "fractional-scale")]
        let fractional_scale_manager: Option<WpFractionalScaleManagerV1> =
            bind_optional(&global_list, &qh, "wp_fractional_scale_manager_v1", 1);
        #[cfg(feature = "fractional-scale")]
        let viewporter: Option<WpViewporter> = bind_optional(&global_list, &qh, "wp_viewporter", 1);

        #[cfg(feature = "xdg-activation")]
        let xdg_activation: Option<XdgActivationV1> =
            bind_optional(&global_list, &qh, "xdg_activation_v1", 1);

        Ok(Connection {
            wl,
            queue,
            globals: Globals {
                compositor,
                subcompositor,
                shm,
                seat,
                xdg_wm_base,
                outputs,
                #[cfg(feature = "layer-shell")]
                layer_shell,
                #[cfg(feature = "text-input")]
                text_input_manager,
                #[cfg(feature = "cursor-shape")]
                cursor_shape_manager,
                #[cfg(feature = "fractional-scale")]
                fractional_scale_manager,
                #[cfg(feature = "fractional-scale")]
                viewporter,
                #[cfg(feature = "xdg-activation")]
                xdg_activation,
            },
        })
    }
}

/// Bind a required global. Logs a friendly error mentioning the
/// interface name if the compositor doesn't advertise it.
fn bind_required<I>(
    global_list: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<State>,
    name: &'static str,
    max_version: u32,
) -> Result<I>
where
    I: wayland_client::Proxy + 'static,
    State: Dispatch<I, ()>,
{
    global_list
        .bind::<I, State, ()>(qh, 1..=max_version, ())
        .map_err(|err| {
            warn!(
                interface = name,
                error = %err,
                "required global not advertised by compositor"
            );
            Error::MissingGlobal { name }
        })
}

/// Bind an optional global. Returns `None` (logged) if the compositor
/// doesn't advertise it.
fn bind_optional<I>(
    global_list: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<State>,
    name: &'static str,
    max_version: u32,
) -> Option<I>
where
    I: wayland_client::Proxy + 'static,
    State: Dispatch<I, ()>,
{
    match global_list.bind::<I, State, ()>(qh, 1..=max_version, ()) {
        Ok(proxy) => Some(proxy),
        Err(err) => {
            debug!(interface = name, error = %err, "optional global not present");
            None
        }
    }
}

// ── Dispatch impls (skeletons — events handled per phase) ────────────────────
//
// wayland-client requires Dispatch impls for every proxy we bind. v0.1
// drops most events on the floor; per-phase tickets add real handling.
// xdg_wm_base.ping → pong is the only one wired here because dropping
// it kills the connection (compositor decides the client is unresponsive).

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &WlConnection,
        _qh: &QueueHandle<Self>,
    ) {
        // wayland-client's `registry_queue_init` consumes the initial
        // global advertisements; subsequent dynamic adds/removes land
        // here. v0.1 ignores them (assume globals don't appear / vanish
        // mid-session). Multi-monitor hot-plug lands in #13.
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _: <WlCompositor as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on wl_compositor in the protocol; nothing to do.
    }
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSubcompositor,
        _: <WlSubcompositor as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSubsurface, SurfaceId> for State {
    fn event(
        _: &mut Self,
        _: &WlSubsurface,
        _: <WlSubsurface as wayland_client::Proxy>::Event,
        _: &SurfaceId,
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // wl_subsurface has no events in the protocol.
    }
}

impl Dispatch<WlShm, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _: <WlShm as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // wl_shm.format events advertise supported pixel formats.
        // v0.1 doesn't render via shm (wgpu's GPU path is preferred);
        // cursor-fallback shm code in #16 will start tracking formats.
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: <WlSeat as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        qh: &QueueHandle<Self>,
    ) {
        if let WlSeatEvent::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        {
            if caps.contains(Capability::Pointer) && state.pointer.wl_pointer.is_none() {
                let pointer = seat.get_pointer(qh, ());
                #[cfg(feature = "cursor-shape")]
                {
                    if let Some(manager) = &state.cursor_shape_manager {
                        state.pointer.cursor_shape_device =
                            Some(manager.get_pointer(&pointer, qh, ()));
                    }
                }
                state.pointer.wl_pointer = Some(pointer);
            }
            if caps.contains(Capability::Keyboard) && state.keyboard.wl_keyboard.is_none() {
                state.keyboard.wl_keyboard = Some(seat.get_keyboard(qh, ()));
            }
            if caps.contains(Capability::Touch) && state.touch.wl_touch.is_none() {
                state.touch.wl_touch = Some(seat.get_touch(qh, ()));
            }
            // text-input-v3 + cursor-shape: bound from `EventLoop::new`
            // (after seat + caps are both known). No work here.
        }
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _keyboard: &WlKeyboard,
        event: <WlKeyboard as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            WlKeyboardEvent::Keymap { format, fd, size } => {
                if !matches!(format, WEnum::Value(KeymapFormat::XkbV1)) {
                    warn!(?format, "ignoring non-xkb keymap format");
                    return;
                }
                // SAFETY: mmap from the fd the compositor handed us;
                // size + format are protocol-controlled and validated
                // above. We immediately copy into a Rust-owned String
                // and release the mapping when xkbcommon is done.
                let keymap_text = match unsafe {
                    use std::os::fd::AsRawFd;
                    let map = libc::mmap(
                        std::ptr::null_mut(),
                        size as usize,
                        libc::PROT_READ,
                        libc::MAP_PRIVATE,
                        fd.as_raw_fd(),
                        0,
                    );
                    if map == libc::MAP_FAILED {
                        None
                    } else {
                        let slice = std::slice::from_raw_parts(
                            map as *const u8,
                            size as usize - 1, // strip trailing NUL
                        );
                        let s = std::str::from_utf8(slice).ok().map(str::to_owned);
                        libc::munmap(map, size as usize);
                        s
                    }
                } {
                    Some(text) => text,
                    None => {
                        warn!("failed to mmap keymap fd");
                        return;
                    }
                };

                let context = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
                let keymap = match xkbcommon::xkb::Keymap::new_from_string(
                    &context,
                    keymap_text,
                    xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1,
                    xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
                ) {
                    Some(km) => km,
                    None => {
                        warn!("xkbcommon failed to parse keymap");
                        return;
                    }
                };
                let xkb_state = xkbcommon::xkb::State::new(&keymap);
                state.keyboard.xkb = Some(XkbState {
                    _context: context,
                    keymap,
                    state: xkb_state,
                });
                debug!("xkb keymap loaded");
            }
            WlKeyboardEvent::Enter { surface, .. } => {
                if let Some(&id) = state.surface_id_by_wl.get(&surface) {
                    state.keyboard.focused_surface = Some(id);
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::Focused,
                    });
                }
            }
            WlKeyboardEvent::Leave { surface, .. } => {
                if let Some(&id) = state.surface_id_by_wl.get(&surface) {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::Unfocused,
                    });
                }
                state.keyboard.focused_surface = None;
                // Stop any in-flight key repeat — the focused surface
                // is gone, the user can't be "holding" anything from
                // our window any more.
                state.keyboard.repeating = None;
            }
            WlKeyboardEvent::Key {
                serial,
                key,
                state: key_state,
                ..
            } => {
                // Track press serial for xdg_activation_token_v1.set_serial.
                // Compositors accept release serials too, but the spec
                // recommends "most recent user input"; press is the
                // canonical "user did a thing just now" signal.
                if matches!(key_state, WEnum::Value(WlKeyState::Pressed)) {
                    state.last_input_serial = serial;
                }
                let surface_id = match state.keyboard.focused_surface {
                    Some(id) => id,
                    None => return,
                };
                let xkb = match state.keyboard.xkb.as_ref() {
                    Some(x) => x,
                    None => return,
                };
                // Wayland sends evdev scancodes (post-X11 +8 offset
                // already applied per protocol; xkbcommon expects
                // exactly that).
                let keycode = xkbcommon::xkb::Keycode::new(key + 8);
                let keysym = xkb.state.key_get_one_sym(keycode);
                let text = xkb.state.key_get_utf8(keycode);
                // xkbcommon returns control characters for Return ("\r"),
                // BackSpace ("\u{8}"), Tab ("\t"), Escape ("\u{1b}"),
                // Delete ("\u{7f}"), and similar — strip those from
                // `text` so consumers can use "text => printable
                // character, key_code => everything else" semantics
                // without having to filter ASCII controls themselves.
                // Matches winit's `KeyEvent::text` behaviour (winit
                // also excludes control characters from `text`).
                let text_opt = if text.is_empty()
                    || (text.chars().count() == 1
                        && text
                            .chars()
                            .next()
                            .is_some_and(|c| (c as u32) < 0x20 || (c as u32) == 0x7f))
                {
                    None
                } else {
                    Some(text)
                };

                let key_name = xkbcommon::xkb::keysym_get_name(keysym);
                let key_code = if !key_name.is_empty() {
                    KeyCode::Named(key_name)
                } else {
                    KeyCode::Sym(keysym.raw())
                };
                let state_variant = match key_state {
                    WEnum::Value(WlKeyState::Pressed) => WayrKeyState::Pressed,
                    WEnum::Value(WlKeyState::Released) => WayrKeyState::Released,
                    _ => return,
                };
                let modifiers = state.keyboard.modifiers;
                // Clone bits we may need for the repeat-state tracker
                // before we move the KeyEvent into pending_events.
                let key_code_clone = key_code.clone();
                let text_clone = text_opt.clone();
                state.pending_events.push(Event::WindowEvent {
                    surface_id,
                    event: WindowEvent::Key(KeyEvent {
                        scancode: ScanCode(key),
                        key_code,
                        modifiers,
                        state: state_variant,
                        text: text_opt,
                        repeat: false,
                    }),
                });

                // Arm / disarm key-repeat tracking based on press/release.
                match state_variant {
                    WayrKeyState::Pressed => {
                        // Only arm if the compositor advertised a
                        // positive rate AND xkbcommon says this key
                        // repeats (modifier keys, lock keys, etc.
                        // typically don't).
                        let key_repeats = xkb.keymap.key_repeats(keycode);
                        if state.keyboard.repeat_rate_hz > 0
                            && state.keyboard.repeat_delay_ms > 0
                            && key_repeats
                        {
                            let delay = std::time::Duration::from_millis(
                                state.keyboard.repeat_delay_ms.max(0) as u64,
                            );
                            state.keyboard.repeating = Some(RepeatingKey {
                                surface_id,
                                scancode: ScanCode(key),
                                key_code: key_code_clone,
                                text: text_clone,
                                next_fire_at: std::time::Instant::now() + delay,
                            });
                        } else {
                            // Non-repeatable key clears any prior
                            // repeat state — pressing a new key
                            // while another was held cancels the
                            // previous repeat.
                            state.keyboard.repeating = None;
                        }
                    }
                    WayrKeyState::Released => {
                        // Only stop repeating if THIS released key is
                        // the one currently set as the repeat source.
                        // Otherwise (typical: modifier release while
                        // a letter is still held), leave the repeat
                        // alone.
                        if state
                            .keyboard
                            .repeating
                            .as_ref()
                            .is_some_and(|r| r.scancode == ScanCode(key))
                        {
                            state.keyboard.repeating = None;
                        }
                    }
                }
            }
            WlKeyboardEvent::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(xkb) = state.keyboard.xkb.as_mut() {
                    xkb.state
                        .update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                    state.keyboard.modifiers = modifiers_from_xkb(&xkb.state, &xkb.keymap);
                }
            }
            WlKeyboardEvent::RepeatInfo { rate, delay } => {
                // `rate` is keys-per-second, `delay` is milliseconds
                // before the first repeat. `rate == 0` per protocol
                // disables key-repeat entirely; we mirror that
                // verbatim so the loop's repeat-synthesis branch
                // becomes inert.
                state.keyboard.repeat_rate_hz = rate;
                state.keyboard.repeat_delay_ms = delay;
                // If a key was already being tracked when the new
                // settings arrived (rare — RepeatInfo usually arrives
                // before any Key), clear it so the next press
                // re-arms with the fresh delay.
                if rate <= 0 || delay <= 0 {
                    state.keyboard.repeating = None;
                }
            }
            _ => {}
        }
    }
}

/// Compute wayr's `Modifiers` from current xkb state. Uses the named
/// modifier API so layout switches (different `Mod1` mappings on
/// non-US layouts) still resolve to the right wayr-level flag.
fn modifiers_from_xkb(state: &xkbcommon::xkb::State, keymap: &xkbcommon::xkb::Keymap) -> Modifiers {
    use xkbcommon::xkb;
    let is = |name: &str| -> bool {
        let idx = keymap.mod_get_index(name);
        if idx == xkb::MOD_INVALID {
            return false;
        }
        state.mod_index_is_active(idx, xkb::STATE_MODS_EFFECTIVE)
    };
    Modifiers {
        shift: is("Shift"),
        ctrl: is("Control"),
        alt: is("Mod1"),
        logo: is("Mod4"),
        caps_lock: is("Lock"),
        num_lock: is("Mod2"),
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _pointer: &WlPointer,
        event: <WlPointer as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            WlPointerEvent::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                state.pointer.enter_serial = serial;
                if let Some(&id) = state.surface_id_by_wl.get(&surface) {
                    state.pointer.focused_surface = Some(id);
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::PointerEntered {
                            position: PointerPosition::from(Position::new(
                                surface_x as i32,
                                surface_y as i32,
                            )),
                        },
                    });
                }
            }
            WlPointerEvent::Leave { surface, .. } => {
                if let Some(&id) = state.surface_id_by_wl.get(&surface) {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::PointerLeft,
                    });
                }
                state.pointer.focused_surface = None;
            }
            WlPointerEvent::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                if let Some(id) = state.pointer.focused_surface {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::PointerMoved {
                            position: PointerPosition::from(Position::new(
                                surface_x as i32,
                                surface_y as i32,
                            )),
                        },
                    });
                }
            }
            WlPointerEvent::Button {
                serial,
                button,
                state: btn_state,
                ..
            } => {
                // Track for xdg_activation_token_v1.set_serial.
                state.last_input_serial = serial;
                if let Some(id) = state.pointer.focused_surface {
                    let pb = evdev_to_pointer_button(button);
                    let pbs = match btn_state {
                        WEnum::Value(WlButtonState::Pressed) => PointerButtonState::Pressed,
                        WEnum::Value(WlButtonState::Released) => PointerButtonState::Released,
                        _ => return,
                    };
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::PointerButton {
                            button: pb,
                            state: pbs,
                            // Keyboard modifier state lives in
                            // wl_keyboard (Phase 1). v0.1 reports
                            // empty modifiers; #10 wires the real
                            // values.
                            modifiers: Modifiers::default(),
                        },
                    });
                }
            }
            WlPointerEvent::Axis { axis, value, .. } => {
                let axis_dir = match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => AxisDirection::Vertical,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => AxisDirection::Horizontal,
                    _ => return,
                };
                match axis_dir {
                    AxisDirection::Vertical => state.pointer.axis_vertical += value,
                    AxisDirection::Horizontal => state.pointer.axis_horizontal += value,
                }
                state.pointer.axis_pending = true;
            }
            WlPointerEvent::AxisDiscrete { axis, discrete } => {
                let axis_dir = match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => AxisDirection::Vertical,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => AxisDirection::Horizontal,
                    _ => return,
                };
                match axis_dir {
                    AxisDirection::Vertical => state.pointer.axis_discrete_v += discrete,
                    AxisDirection::Horizontal => state.pointer.axis_discrete_h += discrete,
                }
                state.pointer.axis_pending = true;
            }
            WlPointerEvent::AxisValue120 { axis, value120 } => {
                let axis_dir = match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => AxisDirection::Vertical,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => AxisDirection::Horizontal,
                    _ => return,
                };
                match axis_dir {
                    AxisDirection::Vertical => state.pointer.axis_value120_v += value120,
                    AxisDirection::Horizontal => state.pointer.axis_value120_h += value120,
                }
                state.pointer.axis_pending = true;
            }
            WlPointerEvent::AxisSource { axis_source } => {
                state.pointer.axis_source = match axis_source {
                    WEnum::Value(wl_pointer::AxisSource::Wheel) => Some(AxisSource::Wheel),
                    WEnum::Value(wl_pointer::AxisSource::Finger) => Some(AxisSource::Finger),
                    WEnum::Value(wl_pointer::AxisSource::Continuous) => {
                        Some(AxisSource::Continuous)
                    }
                    WEnum::Value(wl_pointer::AxisSource::WheelTilt) => Some(AxisSource::WheelTilt),
                    _ => None,
                };
            }
            WlPointerEvent::Frame => {
                // End of an event sequence. Flush accumulated scroll
                // into a single ScrollEvent per axis.
                if !state.pointer.axis_pending {
                    return;
                }
                let id = match state.pointer.focused_surface {
                    Some(id) => id,
                    None => {
                        state.pointer.reset_axis();
                        return;
                    }
                };
                let source = state.pointer.axis_source.unwrap_or(AxisSource::Wheel);
                if state.pointer.axis_vertical != 0.0
                    || state.pointer.axis_discrete_v != 0
                    || state.pointer.axis_value120_v != 0
                {
                    // Wayland's wl_pointer.axis convention is "positive
                    // vertical = scroll down (page goes toward bottom)".
                    // winit (and every other cross-platform windowing
                    // toolkit) normalises this to "positive = scroll up"
                    // so consumers don't have to flip per backend.
                    // Match winit; negate vertical at the emission site.
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::Scroll(ScrollEvent {
                            axis: AxisDirection::Vertical,
                            delta: -state.pointer.axis_vertical,
                            discrete_steps: -state.pointer.axis_discrete_v,
                            high_res_120: -state.pointer.axis_value120_v,
                            source,
                        }),
                    });
                }
                if state.pointer.axis_horizontal != 0.0
                    || state.pointer.axis_discrete_h != 0
                    || state.pointer.axis_value120_h != 0
                {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: id,
                        event: WindowEvent::Scroll(ScrollEvent {
                            axis: AxisDirection::Horizontal,
                            delta: state.pointer.axis_horizontal,
                            discrete_steps: state.pointer.axis_discrete_h,
                            high_res_120: state.pointer.axis_value120_h,
                            source,
                        }),
                    });
                }
                state.pointer.reset_axis();
            }
            // AxisStop / AxisRelativeDirection are refinements wayr
            // doesn't surface yet — they signal kinetic-scroll
            // boundaries and natural-scroll direction respectively.
            _ => {}
        }
    }
}

impl PointerState {
    fn reset_axis(&mut self) {
        self.axis_vertical = 0.0;
        self.axis_horizontal = 0.0;
        self.axis_discrete_v = 0;
        self.axis_discrete_h = 0;
        self.axis_value120_v = 0;
        self.axis_value120_h = 0;
        self.axis_source = None;
        self.axis_pending = false;
    }
}

/// Translate an evdev button code to wayr's [`PointerButton`].
fn evdev_to_pointer_button(code: u32) -> PointerButton {
    match code {
        0x110 => PointerButton::Left,
        0x111 => PointerButton::Right,
        0x112 => PointerButton::Middle,
        0x113 => PointerButton::Back,
        0x114 => PointerButton::Forward,
        other => PointerButton::Other(other),
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: <WlOutput as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // Lazy-allocate an `OutputState` on first event from a given
        // wl_output. Allocates the OutputId once per output for the
        // lifetime of the EventLoop.
        let new_id = if !state.outputs.contains_key(proxy) {
            Some(state.alloc_output_id())
        } else {
            None
        };
        let entry = state.outputs.entry(proxy.clone()).or_default();
        if let Some(id) = new_id {
            entry.id = id;
            entry.scale = 1; // wl_output defaults to 1 until scale arrives.
        }
        match event {
            WlOutputEvent::Geometry { x, y, .. } => {
                entry.position = (x, y);
                entry.ready = false;
            }
            WlOutputEvent::Mode {
                flags,
                width,
                height,
                refresh,
            } => {
                // Compositors may advertise multiple modes (e.g. a
                // user-selectable 60/144/165 Hz list). Only the mode
                // flagged `current` describes the live output state.
                let is_current = matches!(flags, WEnum::Value(f) if f.contains(wayland_client::protocol::wl_output::Mode::Current));
                if is_current {
                    entry.physical_size = Size::new(width.max(0) as u32, height.max(0) as u32);
                    entry.refresh_mhz = refresh;
                    entry.ready = false;
                }
            }
            WlOutputEvent::Scale { factor } => {
                entry.scale = factor.max(1);
                entry.ready = false;
            }
            WlOutputEvent::Name { name } => {
                entry.name = Some(name);
                entry.ready = false;
            }
            WlOutputEvent::Description { description } => {
                entry.description = Some(description);
                entry.ready = false;
            }
            WlOutputEvent::Done => {
                entry.ready = true;
                // Recompute scale for any surface whose touched_outputs
                // includes this one. Cheap O(N_surfaces) — pre-MVP
                // workloads have <10 surfaces.
                let oid = entry.id;
                recompute_scale_for_outputs(state, oid);
            }
            // wl_output v2+ has no other events we care about.
            _ => {}
        }
        // Subpixel + Transform fields exist purely to silence unused
        // imports; they're available to consumers via OutputInfo if we
        // expand the public surface later.
        let _ = (Subpixel::None, Transform::Normal);
    }
}

/// Recompute `scale_factor` for every surface whose `touched_outputs`
/// includes `oid`, emit `ScaleFactorChanged` events when the value
/// actually changed. Called from `wl_output.done` and from the
/// `wl_surface.enter` / `.leave` paths.
fn recompute_scale_for_outputs(state: &mut State, oid: OutputId) {
    let toplevel_ids: Vec<SurfaceId> = state
        .toplevels
        .iter()
        .filter(|(_, st)| st.lock().unwrap().touched_outputs.contains(&oid))
        .map(|(id, _)| *id)
        .collect();
    for sid in toplevel_ids {
        let st_rc = state.toplevels[&sid].clone();
        let new_scale = state.resolved_scale_for_toplevel(&st_rc.lock().unwrap());
        let mut st = st_rc.lock().unwrap();
        if (st.scale_factor - new_scale).abs() > f64::EPSILON {
            st.scale_factor = new_scale;
            let sz = st.current_size;
            drop(st);
            state.pending_events.push(Event::WindowEvent {
                surface_id: sid,
                event: WindowEvent::ScaleFactorChanged {
                    new_scale_factor: new_scale,
                    suggested_size: sz,
                },
            });
        }
    }
    #[cfg(feature = "layer-shell")]
    {
        let ls_ids: Vec<SurfaceId> = state
            .layer_surfaces
            .iter()
            .filter(|(_, st)| st.lock().unwrap().touched_outputs.contains(&oid))
            .map(|(id, _)| *id)
            .collect();
        for sid in ls_ids {
            let st_rc = state.layer_surfaces[&sid].clone();
            let new_scale = state.resolved_scale_for_layer(&st_rc.lock().unwrap());
            let mut st = st_rc.lock().unwrap();
            if (st.scale_factor - new_scale).abs() > f64::EPSILON {
                st.scale_factor = new_scale;
                let sz = st.current_size;
                drop(st);
                state.pending_events.push(Event::WindowEvent {
                    surface_id: sid,
                    event: WindowEvent::ScaleFactorChanged {
                        new_scale_factor: new_scale,
                        suggested_size: sz,
                    },
                });
            }
        }
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _state: &mut Self,
        proxy: &XdgWmBase,
        event: <XdgWmBase as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_protocols::xdg::shell::client::xdg_wm_base::Event as XdgEvent;
        if let XdgEvent::Ping { serial } = event {
            // Required protocol response: failing to pong eventually
            // gets the client killed for unresponsiveness.
            proxy.pong(serial);
        }
    }
}

impl Dispatch<WlSurface, SurfaceId> for State {
    fn event(
        state: &mut Self,
        _surface: &WlSurface,
        event: <WlSurface as wayland_client::Proxy>::Event,
        surface_id: &SurfaceId,
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_surface::Event as WlSurfaceEvent;
        match event {
            WlSurfaceEvent::Enter { output } => {
                // Look up the OutputId for the WlOutput proxy.
                let oid = match state.outputs.get(&output).map(|o| o.id) {
                    Some(id) => id,
                    None => {
                        // wl_output hasn't reported any events yet —
                        // lazy-allocate one now so the surface can
                        // track the membership.
                        let id = state.alloc_output_id();
                        let st = OutputState {
                            id,
                            scale: 1,
                            ..OutputState::default()
                        };
                        state.outputs.insert(output.clone(), st);
                        id
                    }
                };
                update_touched_outputs(state, *surface_id, oid, true);
            }
            WlSurfaceEvent::Leave { output } => {
                if let Some(oid) = state.outputs.get(&output).map(|o| o.id) {
                    update_touched_outputs(state, *surface_id, oid, false);
                }
            }
            WlSurfaceEvent::PreferredBufferScale { factor: _ } => {
                // wl_surface.preferred_buffer_scale (v6) — superseded
                // by wp_fractional_scale_v1 when the manager is
                // present. v0.1 ignores; integer-only consumers rely on
                // the `Enter`/`Leave` path's max-output-scale rule.
            }
            // PreferredBufferTransform — rotation hint, not relevant
            // until we wire wl_surface.set_buffer_transform.
            _ => {}
        }
    }
}

/// Add or remove `oid` from a surface's `touched_outputs` set + emit
/// `ScaleFactorChanged` if the resolved scale changed.
fn update_touched_outputs(state: &mut State, surface_id: SurfaceId, oid: OutputId, enter: bool) {
    if let Some(st_rc) = state.toplevels.get(&surface_id).cloned() {
        {
            let mut st = st_rc.lock().unwrap();
            if enter {
                st.touched_outputs.insert(oid);
            } else {
                st.touched_outputs.remove(&oid);
            }
        }
        let new_scale = state.resolved_scale_for_toplevel(&st_rc.lock().unwrap());
        let mut st = st_rc.lock().unwrap();
        if (st.scale_factor - new_scale).abs() > f64::EPSILON {
            st.scale_factor = new_scale;
            let sz = st.current_size;
            drop(st);
            state.pending_events.push(Event::WindowEvent {
                surface_id,
                event: WindowEvent::ScaleFactorChanged {
                    new_scale_factor: new_scale,
                    suggested_size: sz,
                },
            });
        }
        return;
    }
    #[cfg(not(feature = "layer-shell"))]
    {
        let _ = (state, oid, enter);
    }
    #[cfg(feature = "layer-shell")]
    if let Some(st_rc) = state.layer_surfaces.get(&surface_id).cloned() {
        {
            let mut st = st_rc.lock().unwrap();
            if enter {
                st.touched_outputs.insert(oid);
            } else {
                st.touched_outputs.remove(&oid);
            }
        }
        let new_scale = state.resolved_scale_for_layer(&st_rc.lock().unwrap());
        let mut st = st_rc.lock().unwrap();
        if (st.scale_factor - new_scale).abs() > f64::EPSILON {
            st.scale_factor = new_scale;
            let sz = st.current_size;
            drop(st);
            state.pending_events.push(Event::WindowEvent {
                surface_id,
                event: WindowEvent::ScaleFactorChanged {
                    new_scale_factor: new_scale,
                    suggested_size: sz,
                },
            });
        }
    }
}

impl Dispatch<XdgSurface, SurfaceId> for State {
    fn event(
        state: &mut Self,
        xdg_surface: &XdgSurface,
        event: <XdgSurface as wayland_client::Proxy>::Event,
        surface_id: &SurfaceId,
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        if let XdgSurfaceEvent::Configure { serial } = event {
            // Stash the serial. We ack on the next commit (which the
            // toplevel triggers when ApplicationHandler::resumed
            // returns or when consumer-driven redraw fires). The
            // size landed via xdg_toplevel.configure earlier in the
            // same dispatch round.
            if let Some(tl_state_rc) = state.toplevels.get(surface_id) {
                let mut tl_state = tl_state_rc.lock().unwrap();
                // Ack immediately. wl_surface.commit happens at the
                // end of dispatch in the toplevel.commit() path.
                xdg_surface.ack_configure(serial);
                tl_state.pending_ack = None;

                let new_size = tl_state.current_size;
                let scale = tl_state.scale_factor;
                #[cfg(feature = "fractional-scale")]
                if let Some(vp) = &tl_state.viewport {
                    vp.set_destination(new_size.width.max(1) as i32, new_size.height.max(1) as i32);
                }
                // Dedupe: compositors reconfigure on focus / activated /
                // tiled-state / decoration changes — events with no
                // size or scale delta. Only emit Resized /
                // ScaleFactorChanged when the value actually moved.
                let emit_resized = new_size != tl_state.last_emitted_size;
                let emit_scale = (scale - tl_state.last_emitted_scale).abs() > f64::EPSILON;
                if emit_resized {
                    tl_state.last_emitted_size = new_size;
                }
                if emit_scale {
                    tl_state.last_emitted_scale = scale;
                }
                drop(tl_state);

                if emit_resized {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: *surface_id,
                        event: WindowEvent::Resized(new_size),
                    });
                }
                if emit_scale {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: *surface_id,
                        event: WindowEvent::ScaleFactorChanged {
                            new_scale_factor: scale,
                            suggested_size: new_size,
                        },
                    });
                }
                // RedrawRequested always fires — every configure ack
                // expects a fresh frame attached on the matching
                // commit, regardless of size/scale movement.
                state.pending_events.push(Event::WindowEvent {
                    surface_id: *surface_id,
                    event: WindowEvent::RedrawRequested,
                });
            }
        }
    }
}

impl Dispatch<XdgToplevel, SurfaceId> for State {
    fn event(
        state: &mut Self,
        _toplevel: &XdgToplevel,
        event: <XdgToplevel as wayland_client::Proxy>::Event,
        surface_id: &SurfaceId,
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            XdgToplevelEvent::Configure {
                width,
                height,
                states,
            } => {
                if let Some(tl_state_rc) = state.toplevels.get(surface_id) {
                    let mut tl_state = tl_state_rc.lock().unwrap();

                    // width / height of 0 = compositor leaves the
                    // size up to us; honour the consumer's preferred
                    // size (or the previous current_size, whichever
                    // is non-zero).
                    let w = if width > 0 {
                        width as u32
                    } else if tl_state.current_size.width > 0 {
                        tl_state.current_size.width
                    } else {
                        tl_state.preferred_size.width
                    };
                    let h = if height > 0 {
                        height as u32
                    } else if tl_state.current_size.height > 0 {
                        tl_state.current_size.height
                    } else {
                        tl_state.preferred_size.height
                    };
                    tl_state.current_size = Size::new(w, h);

                    // Track the activated bit on the toplevel for
                    // consumer queries via Toplevel::is_focused (when
                    // we add it), but DON'T emit Focused/Unfocused
                    // here — that's wl_keyboard.enter/leave's job,
                    // which is the authoritative keyboard-focus
                    // source. xdg_toplevel.activated reflects the
                    // "active window" titlebar highlight, which is
                    // related but not identical.
                    let mut activated = false;
                    let mut suspended = false;
                    for raw in states
                        .chunks_exact(4)
                        .filter_map(|chunk| chunk.try_into().ok().map(u32::from_ne_bytes))
                    {
                        if raw == XdgToplevelStateFlag::Activated as u32 {
                            activated = true;
                        } else if raw == XdgToplevelStateFlag::Suspended as u32 {
                            suspended = true;
                        }
                    }
                    tl_state.activated = activated;
                    // Surface Suspended transitions as Occluded so
                    // consumers can pause their idle-repaint timer (CPU
                    // / GPU / battery win when the user tabs away). We
                    // emit only on flip, not on every configure — the
                    // suspended state survives across the many
                    // configures the compositor sends for unrelated
                    // reasons (decoration, tiled-state, …).
                    if tl_state.suspended != suspended {
                        tl_state.suspended = suspended;
                        state.pending_events.push(Event::WindowEvent {
                            surface_id: *surface_id,
                            event: WindowEvent::Occluded(suspended),
                        });
                    }
                    // The Resized event itself is queued by the
                    // matching XdgSurface::Configure handler (which
                    // runs immediately after this one in the same
                    // dispatch round) — that way we only emit one
                    // Resized per configure cycle.
                }
            }
            XdgToplevelEvent::Close => {
                state.pending_events.push(Event::WindowEvent {
                    surface_id: *surface_id,
                    event: WindowEvent::CloseRequested,
                });
            }
            XdgToplevelEvent::ConfigureBounds { .. } => {
                // Hint from the compositor about the max display
                // bounds. v0.1 ignores; consumers that care about
                // max-size adaptation can read it from a future API.
            }
            XdgToplevelEvent::WmCapabilities { .. } => {
                // Compositor advertising which titlebar buttons
                // (minimise / maximise / fullscreen) it implements.
                // v0.1 ignores.
            }
            _ => {}
        }
    }
}

#[cfg(feature = "text-input")]
impl Dispatch<ZwpTextInputManagerV3, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: <ZwpTextInputManagerV3 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on the manager — per-text_input events go to
        // ZwpTextInputV3 below.
    }
}

#[cfg(feature = "text-input")]
impl Dispatch<ZwpTextInputV3, ()> for State {
    fn event(
        state: &mut Self,
        _text_input: &ZwpTextInputV3,
        event: <ZwpTextInputV3 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            TextInputV3Event::Enter { surface } => {
                state.text_input.focused_surface = state.surface_id_by_wl.get(&surface).copied();
            }
            TextInputV3Event::Leave { .. } => {
                state.text_input.focused_surface = None;
                // Reset enabled flag — consumer must re-enable on
                // next focus.
                state.text_input.enabled = false;
            }
            TextInputV3Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                // text-input-v3 spec: text may be null/empty meaning
                // "clear preedit". cursor_begin == cursor_end is the
                // caret position; if either is -1 hide caret.
                let preedit_text = text.unwrap_or_default();
                let cursor = if cursor_begin < 0 || cursor_end < 0 {
                    None
                } else {
                    // wayr's API surfaces a single cursor offset;
                    // the byte range [begin, end) is a selection
                    // hint we collapse to `begin` for now.
                    Some(cursor_begin as u32)
                };
                state.text_input.pending_preedit = Some((preedit_text, cursor));
            }
            TextInputV3Event::CommitString { text } => {
                state.text_input.pending_commit = Some(text.unwrap_or_default());
            }
            TextInputV3Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => {
                state.text_input.pending_delete = Some((before_length, after_length));
            }
            TextInputV3Event::Done { serial } => {
                state.text_input.last_done_serial = serial;
                let surface_id = match state.text_input.focused_surface {
                    Some(id) => id,
                    None => {
                        // Done without focus — drop pending state.
                        state.text_input.pending_preedit = None;
                        state.text_input.pending_commit = None;
                        state.text_input.pending_delete = None;
                        return;
                    }
                };
                // Order matches the text-input-v3 spec's "the order
                // of application is: delete_surrounding_text first,
                // then commit_string, then preedit_string"
                // (preedit replaces existing preedit; commit is the
                // committed text; delete is around the cursor).
                if let Some((before, after)) = state.text_input.pending_delete.take() {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id,
                        event: WindowEvent::Ime(ImeEvent::DeleteSurroundingText {
                            before_bytes: before,
                            after_bytes: after,
                        }),
                    });
                }
                if let Some(text) = state.text_input.pending_commit.take() {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id,
                        event: WindowEvent::Ime(ImeEvent::Commit(text)),
                    });
                }
                if let Some((text, cursor)) = state.text_input.pending_preedit.take() {
                    state.pending_events.push(Event::WindowEvent {
                        surface_id,
                        event: WindowEvent::Ime(ImeEvent::Preedit { text, cursor }),
                    });
                }
            }
            _ => {}
        }
    }
}

#[cfg(feature = "layer-shell")]
impl
    Dispatch<
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
        (),
    > for State
{
    fn event(
        _: &mut Self,
        _: &wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _: <wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on zwlr_layer_shell_v1 itself; per-surface events
        // arrive via zwlr_layer_surface_v1 (below).
    }
}

#[cfg(feature = "layer-shell")]
impl
    Dispatch<
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        SurfaceId,
    > for State
{
    fn event(
        state: &mut Self,
        layer_surface: &wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: <wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1 as wayland_client::Proxy>::Event,
        surface_id: &SurfaceId,
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Event as LsEvent;
        match event {
            LsEvent::Configure {
                serial,
                width,
                height,
            } => {
                if let Some(ls_state_rc) = state.layer_surfaces.get(surface_id) {
                    let mut ls_state = ls_state_rc.lock().unwrap();
                    let w = if width > 0 {
                        width
                    } else if ls_state.current_size.width > 0 {
                        ls_state.current_size.width
                    } else {
                        ls_state.preferred_size.width
                    };
                    let h = if height > 0 {
                        height
                    } else if ls_state.current_size.height > 0 {
                        ls_state.current_size.height
                    } else {
                        ls_state.preferred_size.height
                    };
                    ls_state.current_size = Size::new(w, h);
                    let new_size = ls_state.current_size;
                    let scale = ls_state.scale_factor;
                    #[cfg(feature = "fractional-scale")]
                    if let Some(vp) = &ls_state.viewport {
                        vp.set_destination(
                            new_size.width.max(1) as i32,
                            new_size.height.max(1) as i32,
                        );
                    }
                    // Dedupe (see XdgSurface::Configure handler for the
                    // rationale — same applies to layer-shell).
                    let emit_resized = new_size != ls_state.last_emitted_size;
                    let emit_scale = (scale - ls_state.last_emitted_scale).abs() > f64::EPSILON;
                    if emit_resized {
                        ls_state.last_emitted_size = new_size;
                    }
                    if emit_scale {
                        ls_state.last_emitted_scale = scale;
                    }
                    drop(ls_state);

                    layer_surface.ack_configure(serial);

                    if emit_resized {
                        state.pending_events.push(Event::WindowEvent {
                            surface_id: *surface_id,
                            event: WindowEvent::Resized(new_size),
                        });
                    }
                    if emit_scale {
                        state.pending_events.push(Event::WindowEvent {
                            surface_id: *surface_id,
                            event: WindowEvent::ScaleFactorChanged {
                                new_scale_factor: scale,
                                suggested_size: new_size,
                            },
                        });
                    }
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: *surface_id,
                        event: WindowEvent::RedrawRequested,
                    });
                }
            }
            LsEvent::Closed => {
                if let Some(ls_state_rc) = state.layer_surfaces.get(surface_id) {
                    ls_state_rc.lock().unwrap().closed = true;
                }
                state.pending_events.push(Event::WindowEvent {
                    surface_id: *surface_id,
                    event: WindowEvent::CloseRequested,
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<WlTouch, ()> for State {
    fn event(
        state: &mut Self,
        _touch: &WlTouch,
        event: <WlTouch as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            WlTouchEvent::Down {
                serial,
                surface,
                id,
                x,
                y,
                ..
            } => {
                state.last_input_serial = serial;
                let surface_id = match state.surface_id_by_wl.get(&surface) {
                    Some(&id) => id,
                    None => return,
                };
                let pos = Position::new(x as i32, y as i32);
                state.touch.positions.insert(id, pos);
                state.touch.surfaces.insert(id, surface_id);
                state.pending_events.push(Event::WindowEvent {
                    surface_id,
                    event: WindowEvent::Touch(TouchEvent {
                        id: TouchId(id),
                        phase: TouchPhase::Started,
                        position: pos,
                    }),
                });
            }
            WlTouchEvent::Motion { id, x, y, .. } => {
                let pos = Position::new(x as i32, y as i32);
                state.touch.positions.insert(id, pos);
                let surface_id = match state.touch.surfaces.get(&id) {
                    Some(&s) => s,
                    None => return,
                };
                state.pending_events.push(Event::WindowEvent {
                    surface_id,
                    event: WindowEvent::Touch(TouchEvent {
                        id: TouchId(id),
                        phase: TouchPhase::Moved,
                        position: pos,
                    }),
                });
            }
            WlTouchEvent::Up { id, .. } => {
                let surface_id = match state.touch.surfaces.remove(&id) {
                    Some(s) => s,
                    None => return,
                };
                let pos = state.touch.positions.remove(&id).unwrap_or_default();
                state.pending_events.push(Event::WindowEvent {
                    surface_id,
                    event: WindowEvent::Touch(TouchEvent {
                        id: TouchId(id),
                        phase: TouchPhase::Ended,
                        position: pos,
                    }),
                });
            }
            WlTouchEvent::Cancel => {
                // Cancel all active contacts. Compositor consumed the
                // gesture itself (system swipe etc.).
                let active: Vec<i32> = state.touch.surfaces.keys().copied().collect();
                for id in active {
                    let surface_id = match state.touch.surfaces.remove(&id) {
                        Some(s) => s,
                        None => continue,
                    };
                    let pos = state.touch.positions.remove(&id).unwrap_or_default();
                    state.pending_events.push(Event::WindowEvent {
                        surface_id,
                        event: WindowEvent::Touch(TouchEvent {
                            id: TouchId(id),
                            phase: TouchPhase::Cancelled,
                            position: pos,
                        }),
                    });
                }
            }
            // Frame / Shape / Orientation are batching + finger
            // ergonomics refinements; v0.1 surfaces each Touch event
            // individually and ignores the rest.
            _ => {}
        }
    }
}

#[cfg(feature = "fractional-scale")]
impl Dispatch<WpFractionalScaleManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on the manager.
    }
}

#[cfg(feature = "fractional-scale")]
impl Dispatch<WpFractionalScaleV1, SurfaceId> for State {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: <WpFractionalScaleV1 as wayland_client::Proxy>::Event,
        surface_id: &SurfaceId,
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        if let WpFractionalScaleEvent::PreferredScale { scale } = event {
            // scale is in units of 1/120. Store the integer; resolve
            // to f64 on every recompute so we keep the wire fidelity.
            let new_scale = scale as f64 / 120.0;
            if let Some(st_rc) = state.toplevels.get(surface_id).cloned() {
                let mut st = st_rc.lock().unwrap();
                st.fractional_scale_120 = Some(scale);
                if (st.scale_factor - new_scale).abs() > f64::EPSILON {
                    st.scale_factor = new_scale;
                    let sz = st.current_size;
                    drop(st);
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: *surface_id,
                        event: WindowEvent::ScaleFactorChanged {
                            new_scale_factor: new_scale,
                            suggested_size: sz,
                        },
                    });
                }
                return;
            }
            #[cfg(feature = "layer-shell")]
            if let Some(st_rc) = state.layer_surfaces.get(surface_id).cloned() {
                let mut st = st_rc.lock().unwrap();
                st.fractional_scale_120 = Some(scale);
                if (st.scale_factor - new_scale).abs() > f64::EPSILON {
                    st.scale_factor = new_scale;
                    let sz = st.current_size;
                    drop(st);
                    state.pending_events.push(Event::WindowEvent {
                        surface_id: *surface_id,
                        event: WindowEvent::ScaleFactorChanged {
                            new_scale_factor: new_scale,
                            suggested_size: sz,
                        },
                    });
                }
            }
        }
    }
}

#[cfg(feature = "fractional-scale")]
impl Dispatch<WpViewporter, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events.
    }
}

#[cfg(feature = "fractional-scale")]
impl Dispatch<WpViewport, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events.
    }
}

#[cfg(feature = "cursor-shape")]
impl Dispatch<WpCursorShapeManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeManagerV1,
        _: <WpCursorShapeManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on the manager.
    }
}

#[cfg(feature = "cursor-shape")]
impl Dispatch<WpCursorShapeDeviceV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeDeviceV1,
        _: <WpCursorShapeDeviceV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on the device.
    }
}

// ── xdg_activation_v1 ────────────────────────────────────────────────────────

#[cfg(feature = "xdg-activation")]
impl Dispatch<XdgActivationV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // xdg_activation_v1 carries no client-facing events.
    }
}

#[cfg(feature = "xdg-activation")]
impl Dispatch<XdgActivationTokenV1, ()> for State {
    fn event(
        state: &mut Self,
        token: &XdgActivationTokenV1,
        event: <XdgActivationTokenV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        if let XdgActivationTokenEvent::Done { token: token_str } = event {
            // Resolve the surface this token was created for, fire
            // activate(), and clean up. The activation manager proxy
            // lives on the Connection globals — we re-resolved it via
            // the token's parent rather than storing a clone, which
            // wayland-client doesn't expose. Instead, we recover it
            // from the surface_id_by_wl-shared queue handle via the
            // event_loop accessor at call-site time; here we just hand
            // off to a free helper that looks the proxy up off the
            // proxy id.
            if let Some(surface) = state.pending_activation_tokens.remove(token) {
                // The activation manager proxy must still be live —
                // tokens are short-lived and can only exist when the
                // manager was bound. We re-resolve it by walking the
                // token's parent display via wayland-client's proxy
                // graph. Simpler: stash the manager as a Connection
                // global accessor + clone into State at connect time.
                if let Some(mgr) = state.xdg_activation_manager.as_ref() {
                    mgr.activate(token_str, &surface);
                }
            }
            // Per protocol: token proxies are single-use; destroy on done.
            token.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live-compositor smoke test: connect, bind every required
    /// global, return cleanly. Marked `#[ignore]` because CI runners
    /// don't have a Wayland session — run with
    /// `cargo test -- --ignored` on a real desktop.
    #[test]
    #[ignore]
    fn connect_and_bind_globals() {
        // Permit tracing logs to surface in test output for debugging.
        let _ = tracing_subscriber::fmt::try_init();
        let conn = Connection::connect_to_env().expect("connect to env");
        // Required globals all present.
        let _ = &conn.globals.compositor;
        let _ = &conn.globals.subcompositor;
        let _ = &conn.globals.shm;
        let _ = &conn.globals.seat;
        let _ = &conn.globals.xdg_wm_base;
        // Outputs is a Vec — empty if no monitors connected, but the
        // bind step shouldn't fail.
        assert!(
            !conn.globals.outputs.is_empty(),
            "expected at least one wl_output on a real session"
        );
    }
}
