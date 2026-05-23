//! Output (monitor) state.
//!
//! Each connected display advertises a `wl_output` global. wayr binds
//! all of them at startup, tracks their `geometry` / `mode` / `scale`
//! / `name` / `description` events, and exposes them via
//! [`crate::EventLoop::outputs`] for consumers that want to render
//! into a specific monitor (e.g. pikr's monitor-locked layer-shell).

use crate::geometry::Size;

/// Stable per-output identifier, assigned by wayr at bind time. Match
/// against [`OutputInfo::id`] when consumers want to bind layer-shell
/// surfaces to a particular monitor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OutputId(pub(crate) u64);

impl OutputId {
    /// Raw u64. Stable for the lifetime of the [`crate::EventLoop`].
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Snapshot of a `wl_output`'s state. Returned by
/// [`crate::EventLoop::outputs`].
///
/// `name` + `description` arrive on `wl_output` v4+. On older
/// compositors they stay `None`.
#[derive(Debug, Clone, Default)]
pub struct OutputInfo {
    /// Stable id assigned by wayr.
    pub id: OutputId,
    /// Compositor's machine-readable name (e.g. `"DP-1"`, `"HDMI-A-1"`).
    pub name: Option<String>,
    /// Human-readable description ("Acme 27\"").
    pub description: Option<String>,
    /// Integer scale advertised by the compositor (always at least 1).
    pub scale: i32,
    /// Physical size of the active mode in pixels.
    pub physical_size: Size,
    /// Position in compositor-global coordinates (used by multi-monitor
    /// arrangements). Logical pixels; pre-scale.
    pub position: (i32, i32),
}
