//! Event types emitted by the event loop.

use crate::geometry::Size;
use crate::keyboard::{KeyEvent, Modifiers};
use crate::pointer::{PointerButton, PointerButtonState, PointerPosition, ScrollEvent};
use crate::surface::SurfaceId;
use crate::touch::TouchEvent;

#[cfg(feature = "text-input")]
use crate::ime::ImeEvent;

/// Top-level event the application handler receives.
///
/// `T` is the consumer's user-event type carried by
/// [`crate::EventLoopProxy::send_event`].
#[derive(Debug)]
#[non_exhaustive]
pub enum Event<T> {
    /// First event after the loop starts. Consumers create their
    /// surfaces here (mirrors winit's [`Resumed`] event for ease of
    /// porting).
    ///
    /// [`Resumed`]: https://docs.rs/winit/latest/winit/event/enum.Event.html#variant.Resumed
    Resumed,

    /// Per-surface event. `surface_id` identifies which surface the
    /// event belongs to; consumers track surfaces by ID rather than
    /// holding references across handler calls.
    WindowEvent {
        /// Which surface the event belongs to.
        surface_id: SurfaceId,
        /// What happened.
        event: WindowEvent,
    },

    /// User event dispatched via [`crate::EventLoopProxy::send_event`].
    UserEvent(T),

    /// Event loop is about to wait for new events. Consumers can use
    /// this hook to schedule deferred work or call
    /// [`crate::Surface::request_redraw`] before sleep.
    AboutToWait,

    /// Event loop is shutting down. No further events will arrive.
    LoopExiting,
}

/// Per-surface event variants.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum WindowEvent {
    /// Surface logical size changed. Consumer should resize its
    /// drawing buffer (e.g. recreate the wgpu surface configuration).
    Resized(Size),

    /// Surface scale factor changed (output move, fractional scale).
    /// Consumer should re-render to match the new scale.
    ScaleFactorChanged {
        /// New scale factor (e.g. `1.0`, `1.25`, `1.5`, `2.0`).
        new_scale_factor: f64,
        /// Suggested surface size in logical pixels that preserves the
        /// physical pixel count after the scale change. May be ignored.
        suggested_size: Size,
    },

    /// Compositor requested the surface close (window manager
    /// X-button, layer-shell close request). Consumer decides whether
    /// to honour it.
    CloseRequested,

    /// Compositor told us the surface should redraw now (frame
    /// callback fired after a [`crate::Surface::request_redraw`]).
    RedrawRequested,

    /// Surface gained keyboard focus.
    Focused,

    /// Surface lost keyboard focus.
    Unfocused,

    /// Surface occlusion changed.
    ///
    /// `true` when the compositor has fully obscured the surface
    /// (minimized, covered by an opaque window, off-workspace) — the
    /// consumer should pause idle repaint timers; rendering more pixels
    /// the user can't see is wasted CPU / GPU / battery.
    ///
    /// `false` when the surface becomes visible again — consumer
    /// should resume its normal paint cadence and may want to
    /// `request_redraw()` to refresh stale content.
    ///
    /// Driven by `xdg_toplevel.state.suspended` (xdg-shell v6+). On
    /// compositors that only advertise v5 (or older) the event never
    /// fires; consumers stay in their default "visible" mode.
    Occluded(bool),

    /// Pointer entered the surface.
    PointerEntered {
        /// Initial pointer position.
        position: PointerPosition,
    },

    /// Pointer left the surface.
    PointerLeft,

    /// Pointer moved while inside the surface.
    PointerMoved {
        /// New pointer position.
        position: PointerPosition,
    },

    /// Pointer button transitioned.
    PointerButton {
        /// Which button.
        button: PointerButton,
        /// Pressed or released.
        state: PointerButtonState,
        /// Current modifier state.
        modifiers: Modifiers,
    },

    /// Scroll / wheel event.
    Scroll(ScrollEvent),

    /// Touch event (`wl_touch`). Single contact per event; one
    /// `wl_touch.frame` flushes multiple `Touch` events to the
    /// application handler in order, so multi-touch consumers can
    /// observe the simultaneous state by buffering until the next
    /// non-touch event.
    Touch(TouchEvent),

    /// Keyboard key event.
    Key(KeyEvent),

    /// IME composition event (preedit, commit, surrounding-text
    /// delete). Only emitted when the `text-input` feature is enabled
    /// AND the consumer has called `surface.ime().enable()`.
    #[cfg(feature = "text-input")]
    Ime(ImeEvent),

    /// A committed frame was displayed by the compositor. Fired once
    /// per `wp_presentation_feedback.presented` — i.e. roughly once
    /// per consumer commit on a healthy 60Hz path. Carries the actual
    /// hardware-timestamped presentation time and refresh period;
    /// consumer can use these to schedule the next paint to land just
    /// before the predicted next vblank (low-latency animation,
    /// smooth scroll, vsync-aligned wgpu submit).
    ///
    /// Only emitted when the `presentation-time` feature is on AND
    /// the compositor advertises `wp_presentation`. Always-armed: the
    /// consumer doesn't need to opt in per-frame.
    #[cfg(feature = "presentation-time")]
    FramePresented(crate::PresentationInfo),

    /// A committed frame was discarded by the compositor before
    /// display. Most often: the consumer commit was superseded by a
    /// later commit before the compositor had a chance to scan it
    /// out. Also fires on workspace switches mid-flight.
    ///
    /// Only emitted when the `presentation-time` feature is on AND
    /// the compositor advertises `wp_presentation`.
    #[cfg(feature = "presentation-time")]
    FrameDiscarded,
}
