//! Sub-surface (`wl_subsurface`) child surface.
//!
//! Lands in #12 (Phase 3). Gated behind the `subsurface` feature.
//! Primary consumer: buffr (WPE WebKit native Wayland embedding).
//!
//! A `Subsurface` borrows its parent [`crate::Toplevel`] for the entire
//! lifetime: when the parent is dropped, every child subsurface is
//! invalidated by the protocol, and the borrow ensures consumers can't
//! retain a dangling reference.

use crate::cursor::CursorIcon;
use crate::error::Result;
use crate::geometry::{Position, Rect, Size};
use crate::surface::{RawWindowHandlePlaceholder, Surface, SurfaceId};
use crate::toplevel::Toplevel;

/// A subsurface child of a [`Toplevel`].
///
/// Lifetime `'parent` ties the subsurface to its parent — a
/// `Subsurface<'_>` cannot outlive the parent it was created from.
pub struct Subsurface<'parent> {
    pub(crate) id: SurfaceId,
    pub(crate) _parent: &'parent Toplevel,
    pub(crate) _private: (),
}

impl<'parent> Subsurface<'parent> {
    /// Start building a new subsurface under `parent`.
    pub fn builder(parent: &'parent Toplevel) -> SubsurfaceBuilder<'parent> {
        SubsurfaceBuilder {
            parent,
            position: None,
            size: None,
            sync: true,
        }
    }

    /// Reposition the subsurface relative to its parent's origin.
    ///
    /// Wayland spec: subsurface position is committed atomically with
    /// the *parent* surface's next commit when `sync` mode is active
    /// (the default). Consumer typically calls this in response to a
    /// parent resize and then triggers a parent redraw.
    pub fn set_position(&self, _position: Position) {
        unimplemented!("#12: wl_subsurface.set_position")
    }

    /// Set the subsurface's destination size via `wp_viewport`.
    ///
    /// Without a viewport the subsurface's logical size equals its
    /// buffer size. For embedding scenarios where the buffer (e.g.
    /// WPE WebKit's dma-buf import) doesn't exactly match the layout
    /// rectangle, use this to scale.
    pub fn set_dest_size(&self, _size: Size) {
        unimplemented!("#12: wp_viewport.set_destination")
    }

    /// Convenience for `set_position` + `set_dest_size` in one call.
    pub fn set_geometry(&self, _rect: Rect) {
        unimplemented!("#12")
    }

    /// Place the subsurface immediately above another sibling (or
    /// directly above the parent, if `sibling` is the parent's
    /// surface).
    pub fn place_above(&self, _sibling: &dyn Surface) {
        unimplemented!("#12: wl_subsurface.place_above")
    }

    /// Place the subsurface immediately below another sibling.
    pub fn place_below(&self, _sibling: &dyn Surface) {
        unimplemented!("#12: wl_subsurface.place_below")
    }

    /// Switch to sync mode (subsurface commits roll up into the
    /// parent's next commit — atomic with parent paint).
    pub fn set_sync(&self) {
        unimplemented!("#12: wl_subsurface.set_sync")
    }

    /// Switch to desync mode (subsurface commits are independent of
    /// parent, useful for embedded video / browser engines that paint
    /// faster than the host).
    pub fn set_desync(&self) {
        unimplemented!("#12: wl_subsurface.set_desync")
    }
}

impl Surface for Subsurface<'_> {
    fn id(&self) -> SurfaceId {
        self.id
    }

    fn size(&self) -> Size {
        unimplemented!("#12")
    }

    fn scale_factor(&self) -> f64 {
        unimplemented!("#12 / #13")
    }

    fn request_redraw(&self) {
        unimplemented!("#12")
    }

    fn set_cursor(&self, _icon: CursorIcon) {
        unimplemented!("#16")
    }

    fn raw_window_handle(&self) -> RawWindowHandlePlaceholder {
        unimplemented!(
            "#6: rwh_06 returns the subsurface's wl_surface — \
                       this is what buffr hands to WPE as the embed target"
        )
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

    /// Initial logical size.
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
    pub fn build(self, _event_loop: &mut crate::EventLoop<()>) -> Result<Subsurface<'parent>> {
        unimplemented!("#12: build wl_subsurface")
    }
}
