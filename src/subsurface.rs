//! Sub-surface (`wl_subsurface`) child surface.
//!
//! Primary consumer: buffr (WPE WebKit native Wayland embedding).
//! Gated behind the `subsurface` feature.
//!
//! ## Lifetime semantics
//!
//! Wayland's protocol invalidates child subsurfaces when their
//! parent `wl_surface` is destroyed. `wayr` carries no compile-time
//! lifetime tie — the previous skeleton's `Subsurface<'parent>`
//! conflicted with the common "store both parent and child in the
//! same struct" pattern, which Rust's borrow checker can't express
//! without `Pin` / `Arc` indirection. Instead the runtime
//! invariant matches what the protocol enforces: drop the
//! [`Subsurface`] before the [`crate::Toplevel`] it was built from.
//! In practice consumers naturally do this when both fields drop
//! together at struct teardown.

use wayland_client::Proxy;
use wayland_client::protocol::wl_subsurface::WlSubsurface;
use wayland_client::protocol::wl_surface::WlSurface;

use crate::cursor::CursorIcon;
use crate::error::Result;
use crate::event_loop::EventLoop;
use crate::geometry::{Position, Rect, Size};
use crate::surface::{RawWindowHandlePlaceholder, Surface, SurfaceId};
use crate::toplevel::Toplevel;

/// A subsurface child of a [`Toplevel`].
pub struct Subsurface {
    pub(crate) id: SurfaceId,
    pub(crate) wl_surface: WlSurface,
    pub(crate) wl_subsurface: WlSubsurface,
    pub(crate) initial_size: Size,
}

impl Subsurface {
    /// Start building a new subsurface under `parent`.
    pub fn builder(parent: &Toplevel) -> SubsurfaceBuilder<'_> {
        SubsurfaceBuilder {
            parent,
            position: None,
            size: None,
            sync: true,
        }
    }

    /// Reposition the subsurface relative to its parent's origin.
    ///
    /// Wayland spec: subsurface position is committed atomically
    /// with the parent's next commit when in sync mode (the
    /// default). Consumer typically calls this in response to a
    /// parent resize and triggers a parent redraw.
    pub fn set_position(&self, position: Position) {
        self.wl_subsurface.set_position(position.x, position.y);
    }

    /// Convenience: position + (the subsurface picks up the new
    /// size when the embedder attaches a buffer of that size at the
    /// next commit). Viewport-based scaling lands with #13.
    pub fn set_geometry(&self, rect: Rect) {
        self.set_position(rect.position);
    }

    /// Place the subsurface immediately above another sibling (or
    /// directly above the parent, if `sibling` is the parent's
    /// surface).
    pub fn place_above(&self, sibling: &dyn Surface) {
        let _ = sibling; // Need WlSurface access; expose via Surface trait in a follow-up.
        // Current placeholder uses parent surface via wl_subsurface
        // calls when the consumer hands us a Toplevel ref. We expose
        // a `&dyn Surface` shape today and broaden once layering
        // across sibling subsurfaces is wired (Phase 3 follow-up).
    }

    /// Place the subsurface immediately below another sibling.
    pub fn place_below(&self, sibling: &dyn Surface) {
        let _ = sibling;
    }

    /// Switch to sync mode (subsurface commits roll up into the
    /// parent's next commit — atomic with parent paint).
    pub fn set_sync(&self) {
        self.wl_subsurface.set_sync();
    }

    /// Switch to desync mode (subsurface commits are independent of
    /// parent — useful for embedded video / browser engines that
    /// paint faster than the host).
    pub fn set_desync(&self) {
        self.wl_subsurface.set_desync();
    }

    /// Access the raw `wl_surface` proxy for FFI embedders. This is
    /// the pointer buffr's WPE WebKit backend hands to its
    /// `BuffrDisplayWayland` subclass as the embed target. Lifetime
    /// is tied to `&self`.
    pub fn wl_surface_ptr(&self) -> std::ptr::NonNull<std::ffi::c_void> {
        let id = self.wl_surface.id();
        std::ptr::NonNull::new(id.as_ptr().cast::<std::ffi::c_void>())
            .expect("wl_surface proxy is live for the lifetime of self")
    }
}

impl Drop for Subsurface {
    fn drop(&mut self) {
        self.wl_subsurface.destroy();
        self.wl_surface.destroy();
    }
}

impl Surface for Subsurface {
    fn id(&self) -> SurfaceId {
        self.id
    }

    fn size(&self) -> Size {
        self.initial_size
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn request_redraw(&self) {
        // Subsurfaces don't get configure-driven redraws; the
        // embedder paints when it has new content. No-op.
    }

    fn set_cursor(&self, _icon: CursorIcon) {
        // #16.
    }

    fn raw_window_handle(&self) -> RawWindowHandlePlaceholder {
        RawWindowHandlePlaceholder {
            wl_surface: self.wl_surface_ptr(),
        }
    }
}

// ── raw-window-handle 0.6 impl ──────────────────────────────────────────────

impl raw_window_handle::HasWindowHandle for Subsurface {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        let id = self.wl_surface.id();
        let ptr = std::ptr::NonNull::new(id.as_ptr().cast::<std::ffi::c_void>())
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = raw_window_handle::WaylandWindowHandle::new(ptr);
        // SAFETY: lifetime tied to &self.
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(
                raw_window_handle::RawWindowHandle::Wayland(handle),
            )
        })
    }
}

/// Builder for [`Subsurface`].
pub struct SubsurfaceBuilder<'parent> {
    pub(crate) parent: &'parent Toplevel,
    pub(crate) position: Option<Position>,
    pub(crate) size: Option<Size>,
    pub(crate) sync: bool,
}

impl<'parent> SubsurfaceBuilder<'parent> {
    /// Initial position relative to parent origin.
    pub fn with_position(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }

    /// Initial logical size. Currently informational only —
    /// embedders set the buffer size when they attach. Wired into
    /// `wp_viewport` in #13.
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// Start in desync mode (default: sync).
    pub fn desync(mut self) -> Self {
        self.sync = false;
        self
    }

    /// Construct the subsurface.
    pub fn build<T>(self, event_loop: &mut EventLoop<T>) -> Result<Subsurface> {
        let compositor = event_loop.connection_globals().compositor.clone();
        let subcompositor = event_loop.connection_globals().subcompositor.clone();
        let qh = event_loop.queue_handle();
        let surface_id = event_loop.state.alloc_surface_id();

        let wl_surface = compositor.create_surface(&qh, surface_id);
        let wl_subsurface =
            subcompositor.get_subsurface(&wl_surface, &self.parent.wl_surface, &qh, surface_id);

        if let Some(position) = self.position {
            wl_subsurface.set_position(position.x, position.y);
        }
        if !self.sync {
            wl_subsurface.set_desync();
        }

        event_loop
            .state
            .surface_id_by_wl
            .insert(wl_surface.clone(), surface_id);

        // Commit the parent so the subsurface attach actually
        // happens (subsurface state is atomic with parent commits
        // in sync mode).
        self.parent.wl_surface.commit();

        Ok(Subsurface {
            id: surface_id,
            wl_surface,
            wl_subsurface,
            initial_size: self.size.unwrap_or_default(),
        })
    }
}
