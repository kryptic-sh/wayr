//! Touch input types (`wl_touch`).
//!
//! Each contact is identified by a [`TouchId`] (the protocol-assigned
//! `int` from `wl_touch.down`). IDs are recycled after the matching
//! `up`, so consumers must key their per-contact state off `TouchId`
//! values only for the lifetime of an active contact.

use crate::geometry::Position;

/// Identifier for a single touch contact. Recycled after the contact
/// ends — consumers must drop their per-contact state on
/// [`TouchPhase::Ended`] / [`TouchPhase::Cancelled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TouchId(pub i32);

/// Lifecycle phase of a single touch event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TouchPhase {
    /// Finger / stylus first contacted the surface.
    Started,
    /// Contact moved while still down.
    Moved,
    /// Contact lifted normally.
    Ended,
    /// Compositor cancelled the gesture (e.g. interpreted as a
    /// system-level swipe). Consumers should treat this like
    /// [`TouchPhase::Ended`] but without committing the gesture.
    Cancelled,
}

/// A single touch event.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TouchEvent {
    /// Which contact this event refers to.
    pub id: TouchId,
    /// Phase of this event in the contact's lifecycle.
    pub phase: TouchPhase,
    /// Surface-local position. Always present for `Started` and
    /// `Moved`; carries the last known position for `Ended` /
    /// `Cancelled`.
    pub position: Position,
}
