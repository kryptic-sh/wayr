//! Event loop driver + [`ApplicationHandler`] trait.

use std::os::fd::AsRawFd;
use std::sync::mpsc::{self, SendError};
use std::time::{Duration, Instant};

use wayland_client::{Connection as WlConnection, QueueHandle};

use crate::connection::{Connection, Globals, State};
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
    pub(crate) connection: Connection,
    pub(crate) state: State,
    pub(crate) user_tx: mpsc::Sender<T>,
    pub(crate) user_rx: mpsc::Receiver<T>,
}

impl<T> EventLoop<T> {
    /// Connect to the Wayland compositor advertised by
    /// `WAYLAND_DISPLAY`, bind the required globals, and prepare to
    /// dispatch events.
    pub fn new() -> Result<Self> {
        let connection = Connection::connect_to_env()?;
        let (user_tx, user_rx) = mpsc::channel();
        Ok(Self {
            connection,
            state: State::default(),
            user_tx,
            user_rx,
        })
    }

    /// Get a cheap proxy handle for sending user events from other
    /// threads. The proxy is `Send + Sync`.
    pub fn proxy(&self) -> EventLoopProxy<T> {
        EventLoopProxy {
            tx: self.user_tx.clone(),
        }
    }

    /// Pull the next pending event without blocking. Returns `None`
    /// when no events are queued.
    pub fn poll(&mut self) -> Option<Event<T>> {
        // First, drain any user events synchronously.
        if let Ok(user) = self.user_rx.try_recv() {
            return Some(Event::UserEvent(user));
        }

        // Try non-blocking dispatch of already-buffered events.
        if let Err(err) = self.connection.queue.dispatch_pending(&mut self.state) {
            tracing::warn!(error = %err, "wayland dispatch_pending failed");
        }
        if let Some(pe) = self.state.pending_events.pop() {
            return Some(map_event(pe));
        }

        // Flush + non-blocking read.
        let _ = self.connection.queue.flush();
        if let Some(guard) = self.connection.queue.prepare_read() {
            // Don't block — just return if no data is available.
            let mut pollfd = libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: pollfd is initialised.
            let n = unsafe { libc::poll(&mut pollfd, 1, 0) };
            if n > 0 && (pollfd.revents & libc::POLLIN) != 0 {
                let _ = guard.read();
                let _ = self.connection.queue.dispatch_pending(&mut self.state);
            } else {
                drop(guard);
            }
        }
        self.state.pending_events.pop().map(map_event)
    }

    /// Request the loop exit after the current iteration.
    pub fn exit(&mut self) {
        self.state.exit_requested = true;
    }

    /// Run the event loop blocking. Returns when [`EventLoop::exit`]
    /// is called.
    pub fn run_app(mut self, app: &mut impl ApplicationHandler<T>) -> Result<()> {
        // Deliver Resumed once at startup so the consumer can create
        // surfaces before the first wayland dispatch.
        app.resumed(&mut self);

        loop {
            if self.state.exit_requested {
                app.exiting(&mut self);
                return Ok(());
            }

            // Drain user events (cheap, non-blocking).
            while let Ok(user) = self.user_rx.try_recv() {
                app.user_event(&mut self, user);
                if self.state.exit_requested {
                    app.exiting(&mut self);
                    return Ok(());
                }
            }

            // Flush outgoing requests so the compositor sees our
            // commits / configure acks etc.
            if let Err(err) = self.connection.queue.flush() {
                tracing::warn!(error = %err, "wayland queue flush failed");
            }

            // Blocking pump: wait for incoming socket data or user
            // events. 50ms cap so user-event wakeups don't sleep
            // forever. Bigger budgets are a post-MVP optimisation
            // (eventfd-style wake or calloop integration).
            self.blocking_pump(Duration::from_millis(50));

            // Drain whatever the dispatch produced. `std::mem::take`
            // empties the Vec so we don't keep re-iterating the same
            // events.
            let drained: Vec<Event<()>> = std::mem::take(&mut self.state.pending_events);
            for evt in drained {
                match evt {
                    Event::WindowEvent { surface_id, event } => {
                        app.window_event(&mut self, surface_id, event);
                    }
                    Event::Resumed
                    | Event::UserEvent(())
                    | Event::AboutToWait
                    | Event::LoopExiting => {
                        // Synthesised at well-known points (Resumed
                        // above, AboutToWait below, exit path); the
                        // wayland dispatch path never pushes these.
                    }
                }
                if self.state.exit_requested {
                    app.exiting(&mut self);
                    return Ok(());
                }
            }

            // Fire AboutToWait so consumers can request redraws +
            // deferred work just before sleep.
            app.about_to_wait(&mut self);
        }
    }

    /// Block up to `timeout` waiting for incoming socket data.
    fn blocking_pump(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            // Pull anything already queued.
            let _ = self.connection.queue.dispatch_pending(&mut self.state);
            if !self.state.pending_events.is_empty() {
                return;
            }
            if let Some(guard) = self.connection.queue.prepare_read() {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .unwrap_or_default();
                if remaining.is_zero() {
                    drop(guard);
                    return;
                }
                let mut pollfd = libc::pollfd {
                    fd: guard.connection_fd().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ms = remaining.as_millis().min(50) as i32;
                // SAFETY: pollfd is initialised.
                let n = unsafe { libc::poll(&mut pollfd, 1, ms) };
                if n > 0 && (pollfd.revents & libc::POLLIN) != 0 {
                    let _ = guard.read();
                    let _ = self.connection.queue.dispatch_pending(&mut self.state);
                    return;
                }
                drop(guard);
                if Instant::now() >= deadline {
                    return;
                }
            } else {
                // Already-pending dispatch produced new events; loop
                // around to dispatch_pending.
                continue;
            }
        }
    }

    /// Internal: expose globals to surface builders.
    pub(crate) fn connection_globals(&self) -> &Globals {
        &self.connection.globals
    }

    /// Internal: queue handle for surface builders. Cheap to clone;
    /// builders typically call this once per construction.
    pub(crate) fn queue_handle(&self) -> QueueHandle<State> {
        self.connection.queue.handle()
    }

    /// Internal: wayland `Connection` for `raw-display-handle` etc.
    pub(crate) fn wl_connection(&self) -> &WlConnection {
        &self.connection.wl
    }
}

/// Send-able / clone-able handle for dispatching user events into the
/// loop from other threads.
pub struct EventLoopProxy<T = ()> {
    tx: mpsc::Sender<T>,
}

impl<T> EventLoopProxy<T> {
    /// Send a user event. The event arrives at the handler as
    /// [`Event::UserEvent`] / [`ApplicationHandler::user_event`].
    ///
    /// Returns `Err` if the event loop has already exited.
    pub fn send_event(&self, event: T) -> std::result::Result<(), SendError<T>> {
        self.tx.send(event)
    }
}

impl<T> Clone for EventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

// ── raw-display-handle 0.6 impl for EventLoop (#6) ───────────────────────────

impl<T> raw_window_handle::HasDisplayHandle for EventLoop<T> {
    fn display_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError>
    {
        use wayland_client::Proxy;
        let display = self.connection.wl.display();
        let id = display.id();
        let raw = id.as_ptr();
        let ptr = std::ptr::NonNull::new(raw.cast::<std::ffi::c_void>())
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = raw_window_handle::WaylandDisplayHandle::new(ptr);
        // SAFETY: the handle borrows `self`, so the underlying
        // wl_display lives at least as long as the returned DisplayHandle.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(
                raw_window_handle::RawDisplayHandle::Wayland(handle),
            )
        })
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Convert an internal `Event<()>` (the type wayland dispatch handlers
/// push) into the public `Event<T>`. Only the `WindowEvent` variant
/// carries data wayland produces; the `()` user-event variant is
/// unreachable because the dispatch handlers never push it.
fn map_event<T>(internal: Event<()>) -> Event<T> {
    match internal {
        Event::WindowEvent { surface_id, event } => Event::WindowEvent { surface_id, event },
        Event::Resumed => Event::Resumed,
        Event::AboutToWait => Event::AboutToWait,
        Event::LoopExiting => Event::LoopExiting,
        Event::UserEvent(()) => unreachable!("wayland dispatch never pushes user events"),
    }
}
