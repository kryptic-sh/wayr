//! Cursor shape selection.
//!
//! Concrete implementation in #16 (Phase 7) — prefers
//! `wp_cursor_shape_manager_v1` when the compositor advertises it,
//! falls back to `wl_pointer.set_cursor` + `wl_shm` + theme cursor
//! lookup.
//!
//! The public enum is a re-export of `cursor-icon::CursorIcon` —
//! standardised across the Rust ecosystem (winit, GTK, smithay) so
//! consumers don't need a translation layer.

// In v0.1 we vendor only the variant set we need; pulling the
// `cursor-icon` crate as a dependency is queued for #16. Inlining the
// enum here keeps the scaffold free of native deps until then.

/// Logical cursor shape, mapped to a compositor-provided cursor image.
///
/// Mirrors the W3C `cursor` CSS property + freedesktop cursor names.
/// The compositor or user theme decides the actual visual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CursorIcon {
    /// Standard arrow. Default.
    #[default]
    Default,
    /// Pointing hand (link / clickable).
    Pointer,
    /// Crosshair (precision selection).
    Crosshair,
    /// I-beam (text editing).
    Text,
    /// Loading / busy spinner.
    Wait,
    /// Help / question mark.
    Help,
    /// "No drop here" / forbidden action.
    NotAllowed,
    /// Move / drag.
    Move,
    /// Resize: north-south.
    NsResize,
    /// Resize: east-west.
    EwResize,
    /// Resize: northeast-southwest.
    NeswResize,
    /// Resize: northwest-southeast.
    NwseResize,
    /// Hide the cursor entirely.
    None,
}
