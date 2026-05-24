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
        #[cfg_attr(
            not(any(
                feature = "text-input",
                feature = "cursor-shape",
                feature = "xdg-activation"
            )),
            allow(unused_mut)
        )]
        let mut state = State::default();
        let (user_tx, user_rx) = mpsc::channel();

        // text-input-v3 is a per-seat object; we have both the
        // manager + seat by this point so spawn it eagerly. Surfaces
        // get an `Ime` accessor that mutates this single proxy
        // (text_input follows keyboard focus across surfaces, only
        // one focused at a time per protocol).
        #[cfg(feature = "text-input")]
        if let Some(manager) = &connection.globals.text_input_manager {
            let qh = connection.queue.handle();
            let ti = manager.get_text_input(&connection.globals.seat, &qh, ());
            state.text_input.wp = Some(ti);
        }

        // Cursor-shape manager: cloned into state so the
        // `WlSeat::Capabilities` dispatch can lazy-create the
        // `wp_cursor_shape_device_v1` once the pointer cap arrives.
        #[cfg(feature = "cursor-shape")]
        {
            state.cursor_shape_manager = connection.globals.cursor_shape_manager.clone();
        }

        // xdg_activation_v1 manager: cloned into state so the
        // `XdgActivationTokenV1::Done` dispatch can call
        // `activate(token, surface)` without re-walking Connection.
        #[cfg(feature = "xdg-activation")]
        {
            state.xdg_activation_manager = connection.globals.xdg_activation.clone();
        }

        Ok(Self {
            connection,
            state,
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

    /// Cap the next `blocking_pump` sleep at `deadline`. Single-shot:
    /// the loop honours it for one iteration, then clears the value.
    /// Re-arm from `about_to_wait` each iteration if you want a
    /// persistent deadline.
    ///
    /// Useful for animation pacing — consumers that want a paint at
    /// `t + frame_period` can call `event_loop.wait_until(deadline)`
    /// from `about_to_wait` so the next iteration starts no later
    /// than the deadline (instead of the default 50 ms idle cap).
    /// Real input still preempts the sleep via `poll(2)` — the
    /// deadline only takes effect when no socket activity arrives
    /// before it.
    ///
    /// Calling with a past instant is fine (sleep returns
    /// immediately). The minimum of all internal deadlines + this
    /// value wins, so passing a far-future instant is equivalent to
    /// not calling at all.
    pub fn wait_until(&mut self, deadline: std::time::Instant) {
        self.state.wait_until = Some(match self.state.wait_until {
            Some(prev) => prev.min(deadline),
            None => deadline,
        });
    }

    /// Snapshot of every output currently connected.
    ///
    /// Each entry includes scale, position, physical size, name +
    /// description (when the compositor advertises them). Use the
    /// `OutputId` to compare entries across calls. Consumers should
    /// re-poll on `WindowEvent::ScaleFactorChanged` to pick up any
    /// per-output changes the compositor signalled.
    pub fn outputs(&self) -> Vec<crate::OutputInfo> {
        self.state.outputs.values().map(|o| o.snapshot()).collect()
    }

    /// Set the cursor shape shown over the currently-focused pointer
    /// surface. The compositor only honours the request while one of
    /// our surfaces holds pointer focus (the `enter` serial wayr
    /// remembers must still match).
    ///
    /// No-op if (a) the `cursor-shape` feature is off, (b) the
    /// compositor doesn't advertise `wp_cursor_shape_manager_v1`, or
    /// (c) the seat has no pointer capability yet. Toplevel +
    /// LayerSurface call this from their own `set_cursor` methods.
    #[cfg(feature = "cursor-shape")]
    pub fn set_cursor(&self, icon: crate::CursorIcon) {
        if let Some(dev) = &self.state.pointer.cursor_shape_device {
            dev.set_shape(self.state.pointer.enter_serial, icon.to_protocol());
        }
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

            // Drain any `request_redraw()` flags set during the
            // previous iteration's `window_event` / `about_to_wait`
            // callbacks — synthesize one `RedrawRequested` per
            // flagged surface, then clear the flags. Run before
            // `blocking_pump` so a freshly-flagged redraw doesn't
            // wait the full 50ms when no socket events are arriving.
            self.drain_redraw_requests();

            // Synthesize any due key-repeat events (compositor-paced
            // via wl_keyboard.repeat_info). Run before blocking_pump
            // for the same reason as drain_redraw_requests.
            self.drain_key_repeats();

            // Blocking pump: wait for incoming socket data or user
            // events. Capped to whichever is sooner: 50 ms (so
            // user-event wakeups don't sleep forever), or the next
            // key-repeat fire time. If `drain_*` queued events,
            // blocking_pump returns immediately (its first action
            // checks `pending_events.is_empty()`).
            //
            // Timeout is the minimum of: the 50ms idle cap, the
            // consumer's wait_until deadline (if set during
            // about_to_wait), and the next key-repeat deadline (if
            // any). Internal deadlines reset every iteration; the
            // consumer's wait_until is single-shot.
            let pump_cap = Duration::from_millis(50);
            let now = Instant::now();
            let mut timeout = pump_cap;
            if let Some(rep) = self.state.keyboard.repeating.as_ref() {
                timeout = timeout.min(rep.next_fire_at.saturating_duration_since(now));
            }
            if let Some(deadline) = self.state.wait_until.take() {
                timeout = timeout.min(deadline.saturating_duration_since(now));
            }
            self.blocking_pump(timeout);

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

    /// For each surface whose `needs_redraw` flag is set, push a
    /// synthetic `WindowEvent::RedrawRequested` into the event queue
    /// and clear the flag. Coalesces multiple `request_redraw()` calls
    /// within a single iteration into one event.
    fn drain_redraw_requests(&mut self) {
        // Collect the IDs first so we don't hold a borrow on
        // `self.state.toplevels` while we push into pending_events
        // (both live on `State`).
        let mut to_emit: Vec<crate::SurfaceId> = Vec::new();
        for (sid, st_rc) in self.state.toplevels.iter() {
            let mut st = st_rc.lock().unwrap();
            if st.needs_redraw {
                st.needs_redraw = false;
                to_emit.push(*sid);
            }
        }
        #[cfg(feature = "layer-shell")]
        for (sid, st_rc) in self.state.layer_surfaces.iter() {
            let mut st = st_rc.lock().unwrap();
            if st.needs_redraw {
                st.needs_redraw = false;
                to_emit.push(*sid);
            }
        }
        for sid in to_emit {
            self.state.pending_events.push(Event::WindowEvent {
                surface_id: sid,
                event: WindowEvent::RedrawRequested,
            });
        }
    }

    /// Synthesize a `WindowEvent::Key { repeat: true }` for each
    /// repeat tick that's due, advancing the per-keyboard
    /// `next_fire_at` by `1000 / rate_hz` ms each time. Drives
    /// compositor-paced key-repeat using the `delay` / `rate`
    /// values from `wl_keyboard.repeat_info`.
    fn drain_key_repeats(&mut self) {
        let rate_hz = self.state.keyboard.repeat_rate_hz;
        if rate_hz <= 0 {
            return;
        }
        let period = Duration::from_millis((1000 / rate_hz.max(1)) as u64);
        let modifiers = self.state.keyboard.modifiers;
        let now = Instant::now();
        // Fire 0+ ticks until we've caught up to `now`, then update
        // next_fire_at to the next future deadline. Catch-up is
        // bounded so a long blocking caller can't queue thousands.
        let mut budget: u8 = 32;
        loop {
            let Some(rep) = self.state.keyboard.repeating.as_mut() else {
                return;
            };
            if rep.next_fire_at > now || budget == 0 {
                return;
            }
            budget -= 1;
            let event = crate::WindowEvent::Key(crate::KeyEvent {
                scancode: rep.scancode,
                key_code: rep.key_code.clone(),
                modifiers,
                state: crate::KeyState::Pressed,
                text: rep.text.clone(),
                repeat: true,
            });
            let sid = rep.surface_id;
            rep.next_fire_at += period;
            self.state.pending_events.push(Event::WindowEvent {
                surface_id: sid,
                event,
            });
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

    /// Raw `wl_compositor*` pointer (FFI). Exposed so embedders (e.g.
    /// WPE WebKit via buffr-webkit's `BuffrDisplayWayland` C
    /// subclass) can create their own `wl_surface`s on wayr's
    /// connection. Returns `None` if wayr's compositor proxy is
    /// dead (shouldn't happen for the lifetime of `&self`).
    pub fn wl_compositor_ptr(&self) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
        use wayland_client::Proxy;
        let id = self.connection.globals.compositor.id();
        std::ptr::NonNull::new(id.as_ptr().cast::<std::ffi::c_void>())
    }

    /// Raw `wl_subcompositor*` pointer (FFI). Embedders use this to
    /// create their own `wl_subsurface`s when they own the
    /// embedding decision (vs wayr-managed subsurfaces via
    /// `Subsurface::builder`). buffr's WPE WebKit backend uses
    /// this path because WPE's `BuffrDisplayWayland` subclass
    /// constructs its own subsurface internally.
    pub fn wl_subcompositor_ptr(&self) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
        use wayland_client::Proxy;
        let id = self.connection.globals.subcompositor.id();
        std::ptr::NonNull::new(id.as_ptr().cast::<std::ffi::c_void>())
    }

    /// Raw `wl_display*` pointer (FFI). Same handle that's exposed
    /// via the `raw-display-handle` trait; this convenience version
    /// returns the raw pointer directly for embedders that don't
    /// want to bring `raw-window-handle` into scope.
    pub fn wl_display_ptr(&self) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
        use wayland_client::Proxy;
        let display = self.connection.wl.display();
        let id = display.id();
        std::ptr::NonNull::new(id.as_ptr().cast::<std::ffi::c_void>())
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
