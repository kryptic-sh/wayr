//! Common [`Surface`] trait shared by every surface kind.

use std::num::NonZeroU64;

use crate::geometry::Size;

/// Identifier unique per surface within a single [`crate::EventLoop`].
///
/// Newtype around `NonZeroU64` so consumers can store IDs in compact
/// `Option<SurfaceId>` slots without overhead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(NonZeroU64);

impl SurfaceId {
    /// Construct a `SurfaceId` from a raw `u64`. `0` is reserved and
    /// returns `None`.
    pub fn from_raw(raw: u64) -> Option<Self> {
        NonZeroU64::new(raw).map(Self)
    }

    /// Extract the raw `u64`.
    pub fn as_u64(self) -> u64 {
        self.0.get()
    }
}

/// Shared interface every surface kind implements.
///
/// `Toplevel`, `LayerSurface`, and `Subsurface` all implement
/// `Surface`; consumers that want code generic across surface kinds
/// can program against `&dyn Surface` or `&impl Surface`.
///
/// Methods that are only meaningful on a specific kind (e.g.
/// `set_title` on toplevels, `set_anchor` on layer surfaces,
/// `set_position` on subsurfaces) live on the concrete type, not here.
pub trait Surface {
    /// Stable identifier for matching event-loop events back to this
    /// surface.
    fn id(&self) -> SurfaceId;

    /// Current logical surface size in scale-adjusted pixels.
    fn size(&self) -> Size;

    /// Current effective scale factor (composed output scale +
    /// fractional scale if available). `1.0` until the first
    /// `wl_surface.enter` plus any `wp_fractional_scale_v1` update.
    fn scale_factor(&self) -> f64;

    /// Request the compositor schedule a redraw. The actual paint
    /// happens when [`crate::WindowEvent::RedrawRequested`] is
    /// dispatched, which fires inside the frame callback. Repeated
    /// calls between callbacks coalesce.
    fn request_redraw(&self);

    /// Raw window handle (`wayland-display` + `wl_surface` pointer)
    /// for wgpu / vulkano / glow integration. Lifetime is bound to
    /// `&self` so the handle cannot outlive the surface.
    ///
    /// Concrete `raw-window-handle` traits are implemented in #6 once
    /// the protocol primitives land; this method's signature locks the
    /// surface area for review.
    fn raw_window_handle(&self) -> RawWindowHandlePlaceholder;
}

/// Placeholder for `raw-window-handle::HasWindowHandle`. Replaced with
/// the real `rwh_06::WindowHandle<'_>` borrow in #6.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct RawWindowHandlePlaceholder {
    /// Pointer to the live `wl_surface`. Valid for the lifetime of the
    /// borrowed surface.
    pub wl_surface: std::ptr::NonNull<std::ffi::c_void>,
}

// SAFETY: the pointer is only valid while the borrow on the surface
// lives, but it is otherwise plain data; carrying it across threads is
// caller's responsibility.
unsafe impl Send for RawWindowHandlePlaceholder {}
unsafe impl Sync for RawWindowHandlePlaceholder {}
