//! Wayland-first windowing toolkit for Rust.
//!
//! `wayr` is a minimal, opinionated alternative to winit aimed at
//! [kryptic-sh](https://github.com/kryptic-sh) apps —
//! [buffr](https://github.com/kryptic-sh/buffr),
//! [pikr](https://github.com/kryptic-sh/pikr), and future GUI work.
//! It supports only Wayland on Linux — no X11, macOS, Windows, mobile,
//! or web — and exposes the protocol surfaces those apps actually need
//! (layer-shell, `wl_subsurface` embedding, fractional scaling,
//! text-input-v3, etc.) as first-class API rather than hidden behind
//! raw FFI.
//!
//! # Status
//!
//! Pre-alpha. The public API surface below is locked in shape; the
//! `unimplemented!()` bodies are replaced phase by phase under
//! [umbrella issue #1].
//!
//! [umbrella issue #1]: https://github.com/kryptic-sh/wayr/issues/1
//!
//! # Quick start
//!
//! ```no_run
//! use wayr::{ApplicationHandler, EventLoop, Toplevel, WindowEvent};
//!
//! struct App;
//!
//! impl ApplicationHandler for App {
//!     fn resumed(&mut self, event_loop: &mut EventLoop) {
//!         let _ = Toplevel::builder()
//!             .with_title("hello wayr")
//!             .build(event_loop);
//!     }
//!
//!     fn window_event(
//!         &mut self,
//!         _: &mut EventLoop,
//!         _: wayr::SurfaceId,
//!         event: WindowEvent,
//!     ) {
//!         if matches!(event, WindowEvent::CloseRequested) {
//!             // exit logic
//!         }
//!     }
//! }
//!
//! fn main() -> wayr::Result<()> {
//!     let event_loop = EventLoop::<()>::new()?;
//!     event_loop.run_app(&mut App)
//! }
//! ```
//!
//! # Design
//!
//! See [`docs/design.md`](https://github.com/kryptic-sh/wayr/blob/main/docs/design.md)
//! (added in #17) for the full rationale. Headline decisions:
//!
//! - [`ApplicationHandler`] trait mirrors winit's shape for easy port.
//! - Three concrete surface types — [`Toplevel`], [`LayerSurface`]
//!   (behind `layer-shell` feature), [`Subsurface`] (behind
//!   `subsurface` feature) — all implementing a shared [`Surface`]
//!   trait.
//! - Event loop offers both blocking [`EventLoop::run_app`] and
//!   [`EventLoop::poll`]-based driving.
//! - wgpu / vulkano / glow integration is through `raw-window-handle`
//!   `0.6` only — no GPU crate version coupling.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
// Placeholder fields on builders + private wrappers exist purely to
// reserve names + lifetimes during the API-skeleton phase (#3); the
// real fields land in #4 onwards. Remove this allow once Phase 0
// completes.
#![allow(dead_code)]

mod connection;
mod cursor;
mod error;
mod event;
mod event_loop;
mod geometry;
mod keyboard;
mod output;
mod pointer;
mod surface;
mod toplevel;
mod touch;

#[cfg(feature = "layer-shell")]
mod layer_shell;

#[cfg(feature = "subsurface")]
mod subsurface;

#[cfg(feature = "text-input")]
mod ime;

pub use crate::cursor::CursorIcon;
pub use crate::error::{Error, Result};
pub use crate::event::{Event, WindowEvent};
pub use crate::event_loop::{ApplicationHandler, EventLoop, EventLoopProxy};
pub use crate::geometry::{Position, Rect, Size};
pub use crate::keyboard::{KeyCode, KeyEvent, KeyState, Keymap, Modifiers, RepeatInfo, ScanCode};
pub use crate::output::{OutputId, OutputInfo};
pub use crate::pointer::{
    AxisDirection, AxisSource, PointerButton, PointerButtonState, PointerPosition, ScrollEvent,
};
pub use crate::surface::{RawWindowHandlePlaceholder, Surface, SurfaceId};
pub use crate::toplevel::{Toplevel, ToplevelBuilder};
pub use crate::touch::{TouchEvent, TouchId, TouchPhase};

#[cfg(feature = "layer-shell")]
pub use crate::layer_shell::{
    Anchor, KeyboardInteractivity, Layer, LayerSurface, LayerSurfaceBuilder, Margin,
};

#[cfg(feature = "subsurface")]
pub use crate::subsurface::{Subsurface, SubsurfaceBuilder};

#[cfg(feature = "text-input")]
pub use crate::ime::{ContentHint, ContentPurpose, Ime, ImeEvent};
