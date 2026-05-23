//! Layer-shell (`zwlr_layer_shell_v1`) anchored surfaces.
//!
//! Primary consumer: pikr (anchored picker / dmenu replacement).
//! Gated behind the `layer-shell` feature.

use std::cell::RefCell;
use std::rc::Rc;

use bitflags::bitflags;
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer as WlLayer;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor as WlAnchor, KeyboardInteractivity as WlKeyboardInteractivity, ZwlrLayerSurfaceV1,
};

use crate::connection::LayerSurfaceState;
use crate::cursor::CursorIcon;
use crate::error::{Error, Result};
use crate::event_loop::EventLoop;
use crate::geometry::Size;
use crate::surface::{RawWindowHandlePlaceholder, Surface, SurfaceId};

/// Layer the surface lives on. Layers are Z-stacked in declared order
/// from background (rendered behind everything) to overlay (above
/// regular windows, lock-screen-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Below all regular windows (desktop wallpaper).
    Background,
    /// Between desktop and regular windows.
    Bottom,
    /// Above regular windows (panels, taskbars).
    Top,
    /// Above everything including fullscreen windows (locks, modals).
    Overlay,
}

impl Layer {
    pub(crate) fn to_protocol(self) -> WlLayer {
        match self {
            Layer::Background => WlLayer::Background,
            Layer::Bottom => WlLayer::Bottom,
            Layer::Top => WlLayer::Top,
            Layer::Overlay => WlLayer::Overlay,
        }
    }
}

bitflags! {
    /// Edges of the output the surface anchors to. Combine multiple
    /// edges to span (e.g. `TOP | LEFT | RIGHT` for a top panel).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Anchor: u32 {
        /// Anchor to the top edge.
        const TOP    = 1 << 0;
        /// Anchor to the bottom edge.
        const BOTTOM = 1 << 1;
        /// Anchor to the left edge.
        const LEFT   = 1 << 2;
        /// Anchor to the right edge.
        const RIGHT  = 1 << 3;
    }
}

impl Anchor {
    pub(crate) fn to_protocol(self) -> WlAnchor {
        let mut out = WlAnchor::empty();
        if self.contains(Anchor::TOP) {
            out |= WlAnchor::Top;
        }
        if self.contains(Anchor::BOTTOM) {
            out |= WlAnchor::Bottom;
        }
        if self.contains(Anchor::LEFT) {
            out |= WlAnchor::Left;
        }
        if self.contains(Anchor::RIGHT) {
            out |= WlAnchor::Right;
        }
        out
    }
}

/// How the surface interacts with keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardInteractivity {
    /// Never receives keyboard input.
    None,
    /// Receives keyboard input exclusively (other windows lose focus
    /// while this surface is on screen). Use for modal launchers.
    Exclusive,
    /// Receives keyboard input only when the user clicks into it.
    OnDemand,
}

impl KeyboardInteractivity {
    pub(crate) fn to_protocol(self) -> WlKeyboardInteractivity {
        match self {
            KeyboardInteractivity::None => WlKeyboardInteractivity::None,
            KeyboardInteractivity::Exclusive => WlKeyboardInteractivity::Exclusive,
            KeyboardInteractivity::OnDemand => WlKeyboardInteractivity::OnDemand,
        }
    }
}

/// Margin (in logical pixels) from each anchored edge to the surface.
/// Negative values overhang the edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Margin {
    /// Distance from top edge.
    pub top: i32,
    /// Distance from right edge.
    pub right: i32,
    /// Distance from bottom edge.
    pub bottom: i32,
    /// Distance from left edge.
    pub left: i32,
}

/// A layer-shell surface (`zwlr_layer_surface_v1` on top of `wl_surface`).
pub struct LayerSurface {
    pub(crate) id: SurfaceId,
    pub(crate) wl_surface: WlSurface,
    pub(crate) layer_surface: ZwlrLayerSurfaceV1,
    pub(crate) state: Rc<RefCell<LayerSurfaceState>>,
}

impl LayerSurface {
    /// Start building a new layer-shell surface.
    pub fn builder() -> LayerSurfaceBuilder {
        LayerSurfaceBuilder::default()
    }

    /// Change the anchor edges. Caller must `commit()` (next render
    /// tick) to apply.
    pub fn set_anchor(&self, anchor: Anchor) {
        self.layer_surface.set_anchor(anchor.to_protocol());
    }

    /// Reserve exclusive space along the anchored edge (in logical
    /// pixels). Other clients won't paint into this region. `0` for
    /// "no reservation"; `-1` for "ignore me" (input-only overlay).
    pub fn set_exclusive_zone(&self, zone: i32) {
        self.layer_surface.set_exclusive_zone(zone);
    }

    /// Set margins from each anchored edge.
    pub fn set_margin(&self, margin: Margin) {
        self.layer_surface
            .set_margin(margin.top, margin.right, margin.bottom, margin.left);
    }

    /// Change keyboard interactivity behaviour.
    pub fn set_keyboard_interactivity(&self, ki: KeyboardInteractivity) {
        self.layer_surface
            .set_keyboard_interactivity(ki.to_protocol());
    }

    /// Resize the surface. `0` on an axis means "compositor decides".
    pub fn set_size(&self, size: Size) {
        self.layer_surface.set_size(size.width, size.height);
    }
}

impl Drop for LayerSurface {
    fn drop(&mut self) {
        self.layer_surface.destroy();
        self.wl_surface.destroy();
    }
}

impl Surface for LayerSurface {
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
        // Same caveat as Toplevel: frame-callback driven redraw
        // lands in a Phase 0 follow-up. v0.1 fires RedrawRequested
        // synthetically from the configure ack path.
    }

    fn set_cursor(&self, _icon: CursorIcon) {
        // #16.
    }

    fn raw_window_handle(&self) -> RawWindowHandlePlaceholder {
        let id = self.wl_surface.id();
        let raw = id.as_ptr();
        let ptr = std::ptr::NonNull::new(raw.cast::<std::ffi::c_void>())
            .expect("wl_surface proxy is live for the lifetime of self");
        RawWindowHandlePlaceholder { wl_surface: ptr }
    }
}

// ── raw-window-handle 0.6 impl ──────────────────────────────────────────────

impl raw_window_handle::HasWindowHandle for LayerSurface {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        let id = self.wl_surface.id();
        let raw = id.as_ptr();
        let ptr = std::ptr::NonNull::new(raw.cast::<std::ffi::c_void>())
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = raw_window_handle::WaylandWindowHandle::new(ptr);
        // SAFETY: borrow tied to &self lifetime.
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(
                raw_window_handle::RawWindowHandle::Wayland(handle),
            )
        })
    }
}

/// Builder for [`LayerSurface`].
#[derive(Debug, Default)]
pub struct LayerSurfaceBuilder {
    pub(crate) layer: Option<Layer>,
    pub(crate) anchor: Option<Anchor>,
    pub(crate) size: Option<Size>,
    pub(crate) exclusive_zone: Option<i32>,
    pub(crate) margin: Option<Margin>,
    pub(crate) keyboard_interactivity: Option<KeyboardInteractivity>,
    pub(crate) namespace: Option<String>,
}

impl LayerSurfaceBuilder {
    /// Z-layer the surface sits on.
    pub fn with_layer(mut self, layer: Layer) -> Self {
        self.layer = Some(layer);
        self
    }

    /// Edge anchors.
    pub fn with_anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// Initial surface size. `0` on an axis means "compositor decides"
    /// (typical for panels anchored to both opposing edges).
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// Exclusive space reservation.
    pub fn with_exclusive_zone(mut self, zone: i32) -> Self {
        self.exclusive_zone = Some(zone);
        self
    }

    /// Per-edge margin.
    pub fn with_margin(mut self, margin: Margin) -> Self {
        self.margin = Some(margin);
        self
    }

    /// Keyboard interactivity behaviour.
    pub fn with_keyboard_interactivity(mut self, ki: KeyboardInteractivity) -> Self {
        self.keyboard_interactivity = Some(ki);
        self
    }

    /// `zwlr_layer_shell_v1.namespace` — purely a hint to the
    /// compositor for theming / matching rules (e.g. `"panel"`,
    /// `"launcher"`).
    pub fn with_namespace(mut self, ns: impl Into<String>) -> Self {
        self.namespace = Some(ns.into());
        self
    }

    /// Construct the layer surface.
    pub fn build<T>(self, event_loop: &mut EventLoop<T>) -> Result<LayerSurface> {
        // Clone the proxy refs we need up front so we can mutate
        // `event_loop.state` afterwards without overlapping borrows.
        let layer_shell = event_loop
            .connection_globals()
            .layer_shell
            .as_ref()
            .ok_or(Error::MissingGlobal {
                name: "zwlr_layer_shell_v1",
            })?
            .clone();
        let compositor = event_loop.connection_globals().compositor.clone();
        let qh = event_loop.queue_handle();
        let surface_id = event_loop.state.alloc_surface_id();

        let wl_surface = compositor.create_surface(&qh, surface_id);

        let layer = self.layer.unwrap_or(Layer::Top).to_protocol();
        let namespace = self.namespace.unwrap_or_else(|| "wayr".to_string());
        // output: None → compositor picks the active output.
        let layer_surface =
            layer_shell.get_layer_surface(&wl_surface, None, layer, namespace, &qh, surface_id);

        if let Some(anchor) = self.anchor {
            layer_surface.set_anchor(anchor.to_protocol());
        }
        let size = self.size.unwrap_or(Size::new(0, 0));
        layer_surface.set_size(size.width, size.height);

        if let Some(zone) = self.exclusive_zone {
            layer_surface.set_exclusive_zone(zone);
        }
        if let Some(m) = self.margin {
            layer_surface.set_margin(m.top, m.right, m.bottom, m.left);
        }
        if let Some(ki) = self.keyboard_interactivity {
            layer_surface.set_keyboard_interactivity(ki.to_protocol());
        }

        let state = Rc::new(RefCell::new(LayerSurfaceState {
            current_size: Size::default(),
            preferred_size: size,
            scale_factor: 1.0,
            closed: false,
        }));
        event_loop
            .state
            .layer_surfaces
            .insert(surface_id, Rc::clone(&state));
        event_loop
            .state
            .surface_id_by_wl
            .insert(wl_surface.clone(), surface_id);

        wl_surface.commit();

        Ok(LayerSurface {
            id: surface_id,
            wl_surface,
            layer_surface,
            state,
        })
    }
}
