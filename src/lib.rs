//! Wayland-first windowing toolkit for Rust.
//!
//! `wayr` is a minimal, opinionated alternative to winit aimed at
//! kryptic-sh apps (buffr, pikr, future GUI work). It supports only
//! Wayland on Linux — no X11, macOS, Windows, mobile, or web — and
//! exposes the protocol surfaces those apps actually need (layer-shell,
//! `wl_subsurface` embedding, fractional scaling, text-input-v3, etc.)
//! as first-class API rather than hidden behind raw FFI.
//!
//! # Status
//!
//! Pre-alpha scaffolding. The API surface below will be fleshed out
//! across the phase tickets tracked under [umbrella issue #1].
//!
//! [umbrella issue #1]: https://github.com/kryptic-sh/wayr/issues/1
//!
//! # Design
//!
//! See `docs/design.md` (added in #17) for the full rationale. Headline
//! decisions:
//!
//! - [`ApplicationHandler`] trait mirrors winit's shape for easy port.
//! - Three concrete surface types — `Toplevel`, `LayerSurface` (behind
//!   `layer-shell` feature), `Subsurface` (behind `subsurface` feature)
//!   — all implementing a shared [`Surface`] trait.
//! - Event loop offers both blocking `run_app` and `poll`-based driving.
//! - Integration with wgpu / vulkano / etc. is through `raw-window-handle`
//!   `0.6` only — no wgpu version coupling.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
