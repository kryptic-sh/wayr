//! Presentation-time public types.
//!
//! Wire flow: wayr keeps one `wp_presentation_feedback` per toplevel
//! always armed. The compositor binds each feedback object to the
//! next commit it sees on the surface (whatever code issues that
//! commit — wgpu, vulkano, raw libwayland-client; wayr does not need
//! to intercept). On `presented` / `discarded` the object is
//! destructor-destroyed by the wire protocol; wayr immediately arms a
//! fresh one so the next commit also gets feedback.

use std::time::Duration;

use crate::output::OutputId;

/// Compositor clock domain that `wp_presentation` timestamps live in.
///
/// Wired from the `wp_presentation.clock_id` event. The integer value
/// is the POSIX `CLOCK_*` constant for the chosen clock (almost always
/// `CLOCK_MONOTONIC = 1` in practice; some embedded compositors may
/// pick a different domain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationClock {
    /// Raw `clock_gettime(2)` clock id reported by the compositor.
    pub id: u32,
}

bitflags::bitflags! {
    /// Flags accompanying a [`PresentationInfo`].
    ///
    /// Match the `wp_presentation_feedback.kind` bitfield 1:1 (renamed
    /// for ergonomic Rust naming). Consumers checking for true
    /// vsync alignment want `VSYNC`; latency analysis wants
    /// `HW_CLOCK`; copy-avoidance benchmarks want `ZERO_COPY`.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PresentFlags: u32 {
        /// The presentation was VSYNC'd to the output.
        const VSYNC = 1 << 0;
        /// The timestamp came from a hardware clock.
        const HW_CLOCK = 1 << 1;
        /// The display hardware signalled completion.
        const HW_COMPLETION = 1 << 2;
        /// The wl_buffer was directly scanned out (no compositing copy).
        const ZERO_COPY = 1 << 3;
    }
}

/// Snapshot of one frame's presentation.
///
/// Yielded inside [`crate::WindowEvent::FramePresented`] and cached on
/// each toplevel for synchronous reads via
/// [`crate::Toplevel::last_presented`].
#[derive(Debug, Clone, Copy)]
pub struct PresentationInfo {
    /// Time the content turned into light on the surface's main
    /// output. Expressed in the [`PresentationClock`] domain
    /// (`CLOCK_MONOTONIC` ~always). Use this minus the application's
    /// own paint-start timestamp for end-to-end latency.
    pub time: Duration,
    /// Compositor's prediction of the period between successive
    /// presentations on the synced output. Useful for scheduling the
    /// next paint to land just before the predicted next vblank.
    /// `Duration::ZERO` when the compositor declined to predict.
    pub refresh: Duration,
    /// Monotonic output-relative frame counter (`msc`). Increments by
    /// 1 per refresh; gaps indicate skipped vblanks.
    pub seq: u64,
    /// The output the surface synced to. `None` when the compositor
    /// hasn't reported a sync_output yet (typically the very first
    /// presented event on a freshly-mapped surface). On stable
    /// multi-output setups the compositor pins this for the lifetime
    /// of the surface.
    pub sync_output: Option<OutputId>,
    /// Presentation kind flags. See [`PresentFlags`].
    pub flags: PresentFlags,
}
