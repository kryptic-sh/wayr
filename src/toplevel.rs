//! Top-level (`xdg_toplevel`) window surface.

use crate::cursor::CursorIcon;
use crate::error::Result;
use crate::geometry::Size;
use crate::surface::{RawWindowHandlePlaceholder, Surface, SurfaceId};

/// A regular top-level window (`xdg_toplevel`).
///
/// Created via [`Toplevel::builder`]. Concrete protocol wiring lands
/// in #5.
pub struct Toplevel {
    pub(crate) id: SurfaceId,
    // Real fields land in #5: wl_surface, xdg_surface, xdg_toplevel,
    // pending configure state, current size, scale factor, etc.
    pub(crate) _private: (),
}

impl Toplevel {
    /// Start building a new top-level window.
    pub fn builder() -> ToplevelBuilder {
        ToplevelBuilder::default()
    }

    /// Set the window title (visible in compositor task switchers /
    /// title bars).
    pub fn set_title(&self, _title: impl Into<String>) {
        unimplemented!("#5: xdg_toplevel.set_title")
    }

    /// Set the minimum logical size the compositor is allowed to
    /// resize the surface to. Pass `None` to clear.
    pub fn set_min_size(&self, _size: Option<Size>) {
        unimplemented!("#5: xdg_toplevel.set_min_size")
    }

    /// Set the maximum logical size. Pass `None` for "unbounded".
    pub fn set_max_size(&self, _size: Option<Size>) {
        unimplemented!("#5: xdg_toplevel.set_max_size")
    }

    /// Programmatically request close (compositor-side). Fires the
    /// usual close-window flow, equivalent to the user clicking X.
    pub fn request_close(&self) {
        unimplemented!("#5: xdg_toplevel.destroy")
    }
}

impl Surface for Toplevel {
    fn id(&self) -> SurfaceId {
        self.id
    }

    fn size(&self) -> Size {
        unimplemented!("#5: track current size from configure events")
    }

    fn scale_factor(&self) -> f64 {
        unimplemented!("#5/#13: track output scale + fractional scale")
    }

    fn request_redraw(&self) {
        unimplemented!("#5: wl_surface.frame() + damage")
    }

    fn set_cursor(&self, _icon: CursorIcon) {
        unimplemented!("#16: cursor-shape-v1 or wl_pointer.set_cursor")
    }

    fn raw_window_handle(&self) -> RawWindowHandlePlaceholder {
        unimplemented!("#6: rwh_06 implementation")
    }
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

    /// Construct the top-level window. The actual `wl_surface` /
    /// `xdg_surface` / `xdg_toplevel` are created, but the configure
    /// handshake hasn't completed yet — the surface is not yet
    /// visible. The first
    /// [`crate::WindowEvent::Resized`] arrives once configure resolves.
    pub fn build(self, _event_loop: &mut crate::EventLoop<()>) -> Result<Toplevel> {
        unimplemented!("#5: build the xdg_toplevel surface")
    }
}
