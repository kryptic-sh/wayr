//! Cursor shape selection.
//!
//! v0.1 supports `wp_cursor_shape_manager_v1` (the freedesktop staging
//! protocol that supersedes the legacy theme+shm path). Behind the
//! `cursor-shape` feature; when the compositor doesn't advertise the
//! global, `Toplevel::set_cursor` is a no-op (logged at debug).
//!
//! The legacy `wl_pointer.set_cursor` + `wl_shm` + theme cursor fallback
//! is deliberately omitted — every Wayland compositor wayr targets
//! (KWin ≥5.27, Mutter ≥45, sway with cursor-shape patch, Hyprland,
//! River, Niri) ships cursor-shape support. Apps that need the legacy
//! path should bring their own.

/// Logical cursor shape, mapped to a compositor-provided cursor image.
///
/// Mirrors the W3C `cursor` CSS property + freedesktop cursor names,
/// 1:1 with `wp_cursor_shape_device_v1.shape` enum values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CursorIcon {
    /// Standard arrow.
    #[default]
    Default,
    /// Context menu available.
    ContextMenu,
    /// Help / question mark.
    Help,
    /// Pointing hand (link / clickable).
    Pointer,
    /// Progress indicator (busy but still interactive).
    Progress,
    /// Loading / busy spinner (blocked).
    Wait,
    /// A cell or set of cells may be selected.
    Cell,
    /// Crosshair (precision selection).
    Crosshair,
    /// I-beam (text editing).
    Text,
    /// Vertical text I-beam.
    VerticalText,
    /// Drag-and-drop: alias of / shortcut to something.
    Alias,
    /// Drag-and-drop: copy.
    Copy,
    /// Move / drag.
    Move,
    /// Drag-and-drop: cannot be dropped here.
    NoDrop,
    /// Drag-and-drop: forbidden action.
    NotAllowed,
    /// Drag-and-drop: something can be grabbed.
    Grab,
    /// Drag-and-drop: something is being grabbed.
    Grabbing,
    /// Resize: east border.
    EResize,
    /// Resize: north border.
    NResize,
    /// Resize: north-east corner.
    NeResize,
    /// Resize: north-west corner.
    NwResize,
    /// Resize: south border.
    SResize,
    /// Resize: south-east corner.
    SeResize,
    /// Resize: south-west corner.
    SwResize,
    /// Resize: west border.
    WResize,
    /// Resize: east-west.
    EwResize,
    /// Resize: north-south.
    NsResize,
    /// Resize: north-east-south-west diagonal.
    NeswResize,
    /// Resize: north-west-south-east diagonal.
    NwseResize,
    /// Resize: column (horizontal).
    ColResize,
    /// Resize: row (vertical).
    RowResize,
    /// Scrollable in any direction.
    AllScroll,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
}

#[cfg(feature = "cursor-shape")]
impl CursorIcon {
    /// Map to the wire-protocol shape value. Variant additions stay
    /// in lockstep with `wp_cursor_shape_device_v1`.
    pub(crate) fn to_protocol(
        self,
    ) -> wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape {
        use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
        match self {
            CursorIcon::Default => Shape::Default,
            CursorIcon::ContextMenu => Shape::ContextMenu,
            CursorIcon::Help => Shape::Help,
            CursorIcon::Pointer => Shape::Pointer,
            CursorIcon::Progress => Shape::Progress,
            CursorIcon::Wait => Shape::Wait,
            CursorIcon::Cell => Shape::Cell,
            CursorIcon::Crosshair => Shape::Crosshair,
            CursorIcon::Text => Shape::Text,
            CursorIcon::VerticalText => Shape::VerticalText,
            CursorIcon::Alias => Shape::Alias,
            CursorIcon::Copy => Shape::Copy,
            CursorIcon::Move => Shape::Move,
            CursorIcon::NoDrop => Shape::NoDrop,
            CursorIcon::NotAllowed => Shape::NotAllowed,
            CursorIcon::Grab => Shape::Grab,
            CursorIcon::Grabbing => Shape::Grabbing,
            CursorIcon::EResize => Shape::EResize,
            CursorIcon::NResize => Shape::NResize,
            CursorIcon::NeResize => Shape::NeResize,
            CursorIcon::NwResize => Shape::NwResize,
            CursorIcon::SResize => Shape::SResize,
            CursorIcon::SeResize => Shape::SeResize,
            CursorIcon::SwResize => Shape::SwResize,
            CursorIcon::WResize => Shape::WResize,
            CursorIcon::EwResize => Shape::EwResize,
            CursorIcon::NsResize => Shape::NsResize,
            CursorIcon::NeswResize => Shape::NeswResize,
            CursorIcon::NwseResize => Shape::NwseResize,
            CursorIcon::ColResize => Shape::ColResize,
            CursorIcon::RowResize => Shape::RowResize,
            CursorIcon::AllScroll => Shape::AllScroll,
            CursorIcon::ZoomIn => Shape::ZoomIn,
            CursorIcon::ZoomOut => Shape::ZoomOut,
        }
    }
}
