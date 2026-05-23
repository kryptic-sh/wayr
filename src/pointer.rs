//! Pointer / mouse input types.
//!
//! Pointer event routing lands in #8.

use crate::geometry::Position;

/// Logical pointer button. Wayland speaks Linux evdev codes; `wayr`
/// translates the common ones into named variants and leaves the rest
/// in [`PointerButton::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerButton {
    /// Primary (typically left). evdev `BTN_LEFT` = 0x110.
    Left,
    /// Secondary (typically right). evdev `BTN_RIGHT` = 0x111.
    Right,
    /// Middle / wheel-click. evdev `BTN_MIDDLE` = 0x112.
    Middle,
    /// "Back" thumb button. evdev `BTN_SIDE` = 0x113.
    Back,
    /// "Forward" thumb button. evdev `BTN_EXTRA` = 0x114.
    Forward,
    /// Any other evdev button code.
    Other(u32),
}

/// Whether a pointer button transitioned to pressed or released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButtonState {
    /// Button just pressed.
    Pressed,
    /// Button just released.
    Released,
}

/// Source of an axis (scroll) event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AxisSource {
    /// Discrete scroll wheel (typically integer step per detent).
    Wheel,
    /// Touchpad two-finger scroll (smooth, sub-pixel).
    Finger,
    /// Continuous-motion device (drawing tablet ring, jog dial).
    Continuous,
    /// Tilt of the scroll wheel sideways.
    WheelTilt,
}

/// Scroll axis: vertical or horizontal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisDirection {
    /// Vertical (most common).
    Vertical,
    /// Horizontal (Shift+wheel, touchpad two-finger horizontal).
    Horizontal,
}

/// A scroll / wheel event.
///
/// Wayland separates smooth axis values (`wl_pointer.axis`) from
/// discrete steps (`wl_pointer.axis_discrete` / `axis_value120`). For
/// consumer convenience, `wayr` always emits both fields when
/// available: `delta` is logical pixels, `discrete_steps` is the
/// integer detent count (0 for non-wheel sources).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ScrollEvent {
    /// Which axis scrolled.
    pub axis: AxisDirection,
    /// Smooth delta in logical pixels. Positive = down / right.
    pub delta: f64,
    /// Discrete detent count (0 if source is not [`AxisSource::Wheel`]).
    pub discrete_steps: i32,
    /// What kind of input produced the event.
    pub source: AxisSource,
}

/// Pointer position relative to the surface's origin, in logical
/// (scale-adjusted) pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerPosition(pub Position);

impl From<Position> for PointerPosition {
    fn from(p: Position) -> Self {
        PointerPosition(p)
    }
}
