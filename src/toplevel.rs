//! Top-level (`xdg_toplevel`) window surface.

use std::cell::RefCell;
use std::rc::Rc;

use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;

use crate::connection::ToplevelState;
use crate::error::Result;
use crate::event_loop::EventLoop;
use crate::geometry::Size;
use crate::surface::{RawWindowHandlePlaceholder, Surface, SurfaceId};

/// A regular top-level window (`xdg_toplevel`).
///
/// Created via [`Toplevel::builder`]. Each `Toplevel` owns its protocol
/// objects (`wl_surface`, `xdg_surface`, `xdg_toplevel`); dropping the
/// `Toplevel` destroys them and the compositor unmaps the window.
pub struct Toplevel {
    pub(crate) id: SurfaceId,
    pub(crate) wl_surface: WlSurface,
    pub(crate) xdg_surface: XdgSurface,
    pub(crate) xdg_toplevel: XdgToplevel,
    pub(crate) state: Rc<RefCell<ToplevelState>>,
    /// Per-surface `wp_fractional_scale_v1` listener — present only
    /// when the `fractional-scale` feature is on and the compositor
    /// advertises the manager. Destroyed in `Drop`.
    #[cfg(feature = "fractional-scale")]
    pub(crate) fractional_scale: Option<
        wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1,
    >,
    /// Per-surface `wp_viewport`. Apps that render at fractional scale
    /// drive `set_destination(logical_w, logical_h)` so the compositor
    /// reverse-scales their physical-pixel buffer back to surface
    /// coordinates. Created alongside the fractional-scale listener.
    #[cfg(feature = "fractional-scale")]
    pub(crate) viewport: Option<wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport>,
}

impl Toplevel {
    /// Start building a new top-level window.
    pub fn builder() -> ToplevelBuilder {
        ToplevelBuilder::default()
    }

    /// Set the window title (visible in compositor task switchers /
    /// title bars).
    pub fn set_title(&self, title: impl Into<String>) {
        self.xdg_toplevel.set_title(title.into());
    }

    /// Set the minimum logical size the compositor is allowed to
    /// resize the surface to. Pass `None` to clear.
    pub fn set_min_size(&self, size: Option<Size>) {
        let s = size.unwrap_or_default();
        self.xdg_toplevel
            .set_min_size(s.width as i32, s.height as i32);
    }

    /// Set the maximum logical size. Pass `None` for "unbounded".
    pub fn set_max_size(&self, size: Option<Size>) {
        let s = size.unwrap_or_default();
        self.xdg_toplevel
            .set_max_size(s.width as i32, s.height as i32);
    }

    /// Programmatically request the compositor close this window
    /// (fires the usual close-window flow). The actual destruction
    /// happens on `Toplevel::drop`.
    pub fn request_close(&self) {
        // No xdg_toplevel.close request; the consumer typically drops
        // the `Toplevel` after receiving CloseRequested. This method
        // exists for symmetry / consumer ergonomics — it just sets
        // the `closed` flag so `EventLoop::run_app` knows to exit if
        // no other surfaces are alive.
        self.state.borrow_mut().closed = true;
    }

    /// Access the raw `wl_surface` pointer for FFI consumers (e.g.
    /// embedding a WPE WebKit child via `wl_subsurface`). The
    /// `wayr::Subsurface` API hangs off this; for embedders that
    /// own their own subsurface lifecycle (e.g. WPE's
    /// `BuffrDisplayWayland` subclass) this raw pointer is what
    /// they hand into their constructor as the `parent_wl_surface`.
    pub fn wl_surface_ptr(&self) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
        let id = self.wl_surface.id();
        std::ptr::NonNull::new(id.as_ptr().cast::<std::ffi::c_void>())
    }

    /// Physical buffer size the consumer should render at to match
    /// the surface's current logical size given the active scale
    /// factor. Equivalent to `size() * scale_factor()` rounded up.
    ///
    /// Use this for sizing wgpu / vulkano swapchains: render at
    /// physical resolution, then let the compositor reverse-scale via
    /// the attached `wp_viewport` to the logical surface size.
    pub fn physical_size(&self) -> Size {
        let st = self.state.borrow();
        let s = st.scale_factor.max(1.0);
        Size::new(
            (st.current_size.width as f64 * s).ceil() as u32,
            (st.current_size.height as f64 * s).ceil() as u32,
        )
    }

    /// Set the `wp_viewport` destination, telling the compositor to
    /// treat the attached buffer (rendered at physical resolution) as
    /// covering `size` logical pixels. Auto-applied on configure ack
    /// when the `fractional-scale` feature is on; consumers using
    /// custom render pipelines (e.g. WPE WebKit subsurface embedders)
    /// can call this directly with their preferred logical size.
    #[cfg(feature = "fractional-scale")]
    pub fn set_viewport_destination(&self, size: Size) {
        if let Some(vp) = &self.viewport {
            vp.set_destination(size.width.max(1) as i32, size.height.max(1) as i32);
        }
    }

    /// Set the cursor shape shown when the pointer is over this
    /// surface. Sticky until the next call.
    ///
    /// Wraps [`EventLoop::set_cursor`]; the cursor is per-seat in
    /// wayland (not per-surface), so this method only takes effect
    /// while *this* surface holds pointer focus.
    #[cfg(feature = "cursor-shape")]
    pub fn set_cursor<T>(&self, event_loop: &EventLoop<T>, icon: crate::CursorIcon) {
        event_loop.set_cursor(icon);
    }

    /// IME (text-input-v3) accessor. Returns `None` when the
    /// compositor doesn't advertise `zwp_text_input_manager_v3`
    /// (almost no modern compositors lack it — KWin / Mutter / sway
    /// / Hyprland / River all expose it). Consumer typically calls
    /// `enable()` on focus-into a text field, `disable()` on
    /// focus-out.
    ///
    /// Note: text-input-v3 is per-seat, not per-surface — calls on
    /// an unfocused surface's `Ime` accessor are silently ignored
    /// by the compositor until focus returns. The accessor exists
    /// per surface for ergonomic consistency with other surface
    /// methods.
    #[cfg(feature = "text-input")]
    pub fn ime<T>(&self, event_loop: &crate::EventLoop<T>) -> Option<crate::Ime> {
        event_loop
            .state
            .text_input
            .wp
            .as_ref()
            .map(|wp| crate::Ime { wp: wp.clone() })
    }
}

impl Drop for Toplevel {
    fn drop(&mut self) {
        // Order matters: destroy the wp_* extension objects first, then
        // xdg_toplevel → xdg_surface → wl_surface (reverse of
        // construction). Wayland-client destroys the proxy when it
        // goes out of scope, but each layer above has an explicit
        // `destroy` request that must be sent first.
        #[cfg(feature = "fractional-scale")]
        {
            if let Some(fs) = self.fractional_scale.take() {
                fs.destroy();
            }
            if let Some(vp) = self.viewport.take() {
                vp.destroy();
            }
        }
        self.xdg_toplevel.destroy();
        self.xdg_surface.destroy();
        self.wl_surface.destroy();
    }
}

impl Surface for Toplevel {
    fn id(&self) -> SurfaceId {
        self.id
    }

    fn size(&self) -> Size {
        self.state.borrow().current_size
    }

    fn scale_factor(&self) -> f64 {
        self.state.borrow().scale_factor
    }

    fn request_redraw(&self) {
        // Flag the surface for redraw on the next run-loop iteration.
        // The run_app loop synthesizes a single
        // `WindowEvent::RedrawRequested` per surface per iteration even
        // if `request_redraw` was called multiple times — matching
        // winit's coalescing semantics. wl_surface.frame() compositor-
        // paced redraws are queued for a future release; this immediate
        // path is sufficient for consumers that want a
        // synchronously-driven repaint (e.g. buffr's
        // input → request_redraw → paint flow).
        self.state.borrow_mut().needs_redraw = true;
    }

    fn raw_window_handle(&self) -> RawWindowHandlePlaceholder {
        let ptr = wl_proxy_ptr(&self.wl_surface)
            .expect("wl_surface proxy is live for the lifetime of self");
        RawWindowHandlePlaceholder { wl_surface: ptr }
    }
}

// ── raw-window-handle 0.6 impl (#6) ──────────────────────────────────────────

impl raw_window_handle::HasWindowHandle for Toplevel {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        let ptr =
            wl_proxy_ptr(&self.wl_surface).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = raw_window_handle::WaylandWindowHandle::new(ptr);
        // SAFETY: the handle borrows `self`, so the underlying
        // wl_surface lives at least as long as the returned WindowHandle.
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(
                raw_window_handle::RawWindowHandle::Wayland(handle),
            )
        })
    }
}

/// Internal helper: extract a `NonNull<c_void>` pointer to the C
/// `wl_proxy*` for a wayland-client proxy. Returns `None` if the
/// proxy is already dead (shouldn't happen while holding `&Toplevel`
/// since we own the proxy).
fn wl_proxy_ptr<P: Proxy>(proxy: &P) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    let id = proxy.id();
    let raw = id.as_ptr();
    std::ptr::NonNull::new(raw.cast::<std::ffi::c_void>())
}

/// Builder for [`Toplevel`].
///
/// Defaults: title = empty string, app_id = `CARGO_PKG_NAME` of the
/// consumer, initial size = `(800, 600)`, no min/max constraints.
#[derive(Debug, Default)]
pub struct ToplevelBuilder {
    pub(crate) title: Option<String>,
    pub(crate) app_id: Option<String>,
    pub(crate) initial_size: Option<Size>,
    pub(crate) min_size: Option<Size>,
    pub(crate) max_size: Option<Size>,
}

impl ToplevelBuilder {
    /// Set the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the `xdg_toplevel.set_app_id`. Compositors group windows
    /// with the same app_id together (e.g. in task switchers); for a
    /// consistent grouping, set this to your reverse-DNS bundle id
    /// (e.g. `"sh.kryptic.buffr"`).
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }

    /// Set the initial logical surface size. The compositor may
    /// override during the configure handshake; this is a hint.
    pub fn with_initial_size(mut self, size: Size) -> Self {
        self.initial_size = Some(size);
        self
    }

    /// Set the minimum logical size the compositor is allowed to
    /// resize to.
    pub fn with_min_size(mut self, size: Size) -> Self {
        self.min_size = Some(size);
        self
    }

    /// Set the maximum logical size.
    pub fn with_max_size(mut self, size: Size) -> Self {
        self.max_size = Some(size);
        self
    }

    /// Construct the top-level window. The `wl_surface` /
    /// `xdg_surface` / `xdg_toplevel` are created and committed
    /// empty (which kicks off the configure handshake). The first
    /// [`crate::WindowEvent::Resized`] / [`crate::WindowEvent::RedrawRequested`]
    /// arrives once the compositor's configure resolves.
    pub fn build<T>(self, event_loop: &mut EventLoop<T>) -> Result<Toplevel> {
        let initial_size = self.initial_size.unwrap_or(Size::new(800, 600));
        let surface_id = event_loop.state.alloc_surface_id();
        let qh = event_loop.queue_handle();

        // wl_compositor.create_surface — first half of the
        // surface-creation chain. UserData is the SurfaceId so
        // dispatch can find the matching toplevel state.
        let wl_surface = event_loop
            .connection_globals()
            .compositor
            .create_surface(&qh, surface_id);

        // xdg_wm_base.get_xdg_surface(wl_surface) — adds the
        // xdg-shell role on top.
        let xdg_surface = event_loop.connection_globals().xdg_wm_base.get_xdg_surface(
            &wl_surface,
            &qh,
            surface_id,
        );

        // xdg_surface.get_toplevel() — adds the toplevel sub-role.
        let xdg_toplevel = xdg_surface.get_toplevel(&qh, surface_id);

        if let Some(title) = self.title {
            xdg_toplevel.set_title(title);
        }
        let app_id = self
            .app_id
            .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());
        xdg_toplevel.set_app_id(app_id);

        if let Some(min) = self.min_size {
            xdg_toplevel.set_min_size(min.width as i32, min.height as i32);
        }
        if let Some(max) = self.max_size {
            xdg_toplevel.set_max_size(max.width as i32, max.height as i32);
        }

        // Attach fractional-scale + viewport extensions if available.
        #[cfg(feature = "fractional-scale")]
        let fractional_scale = event_loop
            .connection_globals()
            .fractional_scale_manager
            .as_ref()
            .map(|m| m.get_fractional_scale(&wl_surface, &qh, surface_id));
        #[cfg(feature = "fractional-scale")]
        let viewport = event_loop
            .connection_globals()
            .viewporter
            .as_ref()
            .map(|v| v.get_viewport(&wl_surface, &qh, ()));

        // Register state map entry before the first commit so the
        // configure dispatch can find it.
        let state = Rc::new(RefCell::new(ToplevelState {
            current_size: Size::default(),
            preferred_size: initial_size,
            pending_ack: None,
            closed: false,
            activated: false,
            scale_factor: 1.0,
            needs_redraw: false,
            fractional_scale_120: None,
            touched_outputs: Default::default(),
            #[cfg(feature = "fractional-scale")]
            viewport: viewport.clone(),
        }));
        event_loop
            .state
            .toplevels
            .insert(surface_id, Rc::clone(&state));
        event_loop
            .state
            .surface_id_by_wl
            .insert(wl_surface.clone(), surface_id);

        // Empty commit kicks off the configure cycle.
        wl_surface.commit();

        Ok(Toplevel {
            id: surface_id,
            wl_surface,
            xdg_surface,
            xdg_toplevel,
            state,
            #[cfg(feature = "fractional-scale")]
            fractional_scale,
            #[cfg(feature = "fractional-scale")]
            viewport,
        })
    }
}
