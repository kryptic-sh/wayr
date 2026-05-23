//! Event loop driver + [`ApplicationHandler`] trait.

use std::sync::mpsc::{self, RecvError, SendError};

use crate::error::Result;
use crate::event::{Event, WindowEvent};
use crate::surface::SurfaceId;

/// Application-side hook called by the event loop. Mirrors winit's
/// [`ApplicationHandler`] shape so consumer ports are mechanical.
///
/// All methods have safe no-op defaults; implement only what the app
/// uses.
///
/// [`ApplicationHandler`]: https://docs.rs/winit/latest/winit/application/trait.ApplicationHandler.html
pub trait ApplicationHandler<T = ()> {
    /// Called once after [`EventLoop::run_app`] starts. Consumers
    /// create their toplevel / layer-shell surfaces here.
    fn resumed(&mut self, _event_loop: &mut EventLoop<T>) {}

    /// Per-surface event.
    fn window_event(
        &mut self,
        _event_loop: &mut EventLoop<T>,
        _surface_id: SurfaceId,
        _event: WindowEvent,
    ) {
    }

    /// User event dispatched via [`EventLoopProxy::send_event`].
    fn user_event(&mut self, _event_loop: &mut EventLoop<T>, _event: T) {}

    /// Loop is about to block waiting for new events. Schedule
    /// deferred work or call [`crate::Surface::request_redraw`] before
    /// sleep.
    fn about_to_wait(&mut self, _event_loop: &mut EventLoop<T>) {}

    /// Loop is shutting down. No further callbacks will fire.
    fn exiting(&mut self, _event_loop: &mut EventLoop<T>) {}
}

/// Event loop owning the Wayland connection + every surface.
///
/// `T` is the user-event type carried by [`EventLoopProxy::send_event`].
/// Default `T = ()` for the common case where no user events are
/// needed.
pub struct EventLoop<T = ()> {
    pub(crate) _user_tx: mpsc::Sender<T>,
    pub(crate) _user_rx: mpsc::Receiver<T>,
    // Real fields land with #4 / #7: wayland-client Connection, the
    // calloop EventLoop, the registry, bound globals, per-surface
    // state map, the exit flag.
    pub(crate) _private: (),
}

impl<T> EventLoop<T> {
    /// Connect to the Wayland compositor advertised by
    /// `WAYLAND_DISPLAY`, bind the required globals, and prepare to
    /// dispatch events.
    pub fn new() -> Result<Self> {
        unimplemented!("#4 + #7")
    }

    /// Get a cheap proxy handle for sending user events from other
    /// threads. The proxy is `Send + Sync`.
    pub fn proxy(&self) -> EventLoopProxy<T> {
        unimplemented!("#7")
    }

    /// Run the event loop blocking. Returns when the consumer calls
    /// [`EventLoop::exit`] or every surface has been closed and the
    /// consumer hasn't created new ones.
    pub fn run_app(self, _app: &mut impl ApplicationHandler<T>) -> Result<()> {
        unimplemented!("#7: calloop-driven dispatch")
    }

    /// Pull the next event without blocking. Returns `None` when no
    /// events are queued. Useful for tests and for tooling that needs
    /// to drive the loop manually (e.g. integration with another
    /// event source). Most apps use [`EventLoop::run_app`].
    pub fn poll(&mut self) -> Option<Event<T>> {
        unimplemented!("#7")
    }

    /// Request the loop exit after returning the current event.
    /// Subsequent [`EventLoop::poll`] / [`EventLoop::run_app`]
    /// iterations will emit [`Event::LoopExiting`] and stop.
    pub fn exit(&mut self) {
        unimplemented!("#7")
    }
}

/// Send-able / clone-able handle for dispatching user events into the
/// loop from other threads.
pub struct EventLoopProxy<T = ()> {
    pub(crate) _tx: mpsc::Sender<T>,
}

impl<T> EventLoopProxy<T> {
    /// Send a user event. The event arrives at the handler as
    /// [`Event::UserEvent`] / [`ApplicationHandler::user_event`].
    ///
    /// Returns `Err` if the event loop has already exited.
    pub fn send_event(&self, event: T) -> std::result::Result<(), SendError<T>> {
        self._tx.send(event)
    }
}

impl<T> Clone for EventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self {
            _tx: self._tx.clone(),
        }
    }
}

// Allows blocking-recv on the rx side without leaking internals.
pub(crate) fn _recv<T>(rx: &mpsc::Receiver<T>) -> std::result::Result<T, RecvError> {
    rx.recv()
}
