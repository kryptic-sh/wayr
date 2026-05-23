//! Top-level (`xdg_toplevel`) window surface.

use std::cell::RefCell;
use std::rc::Rc;

use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;

use crate::connection::ToplevelState;
use crate::cursor::CursorIcon;
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
    /// embedding a WPE WebKit child via `wl_subsurface`). Subsurface
    /// API hangs off this (#12).
    pub fn wl_surface_id(&self) -> u32 {
        self.wl_surface.id().protocol_id()
    }
}

impl Drop for Toplevel {
    fn drop(&mut self) {
        // Order matters: destroy xdg_toplevel → xdg_surface →
        // wl_surface (reverse of construction). Wayland-client
        // destroys the proxy when it goes out of scope, but
        // xdg_toplevel + xdg_surface have explicit `destroy`
        // requests that must be sent first.
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
        // Schedule a frame callback. The compositor delivers a
        // wl_callback.done event which the dispatch handler converts
        // into a WindowEvent::RedrawRequested. v0.1 fires
        // RedrawRequested synthetically from the configure ack path,
        // so this is a no-op for now; #5/#7 wiring of frame callbacks
        // lands in a follow-up.
    }

    fn set_cursor(&self, _icon: CursorIcon) {
        // Lands in #16.
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

        // Register state map entry before the first commit so the
        // configure dispatch can find it.
        let state = Rc::new(RefCell::new(ToplevelState {
            current_size: Size::default(),
            preferred_size: initial_size,
            pending_ack: None,
            closed: false,
            activated: false,
            scale_factor: 1.0,
        }));
        event_loop
            .state
            .toplevels
            .insert(surface_id, Rc::clone(&state));

        // Empty commit kicks off the configure cycle.
        wl_surface.commit();

        Ok(Toplevel {
            id: surface_id,
            wl_surface,
            xdg_surface,
            xdg_toplevel,
            state,
        })
    }
}
