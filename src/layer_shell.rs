//! Layer-shell (`zwlr_layer_shell_v1`) anchored surfaces.
//!
//! Lands in #11 (Phase 2). Gated behind the `layer-shell` feature.
//! Primary consumer: pikr (anchored picker / dmenu replacement).

use bitflags::bitflags;

use crate::cursor::CursorIcon;
use crate::error::Result;
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
    pub(crate) _private: (),
}

impl LayerSurface {
    /// Start building a new layer-shell surface.
    pub fn builder() -> LayerSurfaceBuilder {
        LayerSurfaceBuilder::default()
    }

    /// Change the anchor edges. Triggers a reconfigure.
    pub fn set_anchor(&self, _anchor: Anchor) {
        unimplemented!("#11: zwlr_layer_surface_v1.set_anchor")
    }

    /// Reserve exclusive space along the anchored edge (in logical
    /// pixels). Other clients won't paint into this region. Use `0`
    /// for "no reservation". Pass `-1` for "ignore me" (input-only
    /// overlay).
    pub fn set_exclusive_zone(&self, _zone: i32) {
        unimplemented!("#11: zwlr_layer_surface_v1.set_exclusive_zone")
    }

    /// Set margins from each anchored edge.
    pub fn set_margin(&self, _margin: Margin) {
        unimplemented!("#11: zwlr_layer_surface_v1.set_margin")
    }

    /// Change keyboard interactivity behaviour.
    pub fn set_keyboard_interactivity(&self, _ki: KeyboardInteractivity) {
        unimplemented!("#11: zwlr_layer_surface_v1.set_keyboard_interactivity")
    }
}

impl Surface for LayerSurface {
    fn id(&self) -> SurfaceId {
        self.id
    }

    fn size(&self) -> Size {
        unimplemented!("#11")
    }

    fn scale_factor(&self) -> f64 {
        unimplemented!("#11")
    }

    fn request_redraw(&self) {
        unimplemented!("#11")
    }

    fn set_cursor(&self, _icon: CursorIcon) {
        unimplemented!("#16")
    }

    fn raw_window_handle(&self) -> RawWindowHandlePlaceholder {
        unimplemented!("#6")
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
    pub(crate) output: Option<()>, // wl_output ref; #11 fleshes out
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
    pub fn build(self, _event_loop: &mut crate::EventLoop<()>) -> Result<LayerSurface> {
        unimplemented!("#11: build zwlr_layer_surface_v1")
    }
}
