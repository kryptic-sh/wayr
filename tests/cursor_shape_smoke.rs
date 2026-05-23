//! Integration test: cursor-shape feature scaffolding doesn't blow up
//! against a real compositor. We can't observe a cursor visually from
//! headless sway, but we can: (a) connect with `cursor-shape` on,
//! (b) build a toplevel, (c) call `set_cursor` and confirm the
//! request flushes without protocol error.
//!
//! `#[ignore]` because this needs a live Wayland session — wired into
//! tests/run-e2e.sh which spawns headless sway.

#![cfg(feature = "cursor-shape")]

use std::time::{Duration, Instant};

use wayr::{ApplicationHandler, CursorIcon, EventLoop, Size, SurfaceId, Toplevel, WindowEvent};

#[derive(Default)]
struct App {
    window: Option<Toplevel>,
    started_at: Option<Instant>,
    saw_configure: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &mut EventLoop) {
        if self.window.is_some() {
            return;
        }
        let toplevel = Toplevel::builder()
            .with_title("wayr cursor-shape smoke")
            .with_initial_size(Size::new(640, 480))
            .build(event_loop)
            .expect("build toplevel");
        // Call set_cursor before the pointer focuses us — should be a
        // no-op (no enter_serial yet) rather than a protocol error.
        toplevel.set_cursor(event_loop, CursorIcon::Pointer);
        self.window = Some(toplevel);
        self.started_at = Some(Instant::now());
    }

    fn window_event(
        &mut self,
        event_loop: &mut EventLoop,
        _surface_id: SurfaceId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::RedrawRequested) {
            self.saw_configure = true;
            // After configure, exercise every cursor variant to make
            // sure the protocol mapper is wire-correct. The serial we
            // pass is `0` until the compositor sends pointer.enter
            // (which never happens under headless sway with no input
            // device), so the compositor silently ignores these — but
            // the wire encoding still goes out and a bad enum value
            // would trip a protocol error here.
            if let Some(window) = &self.window {
                for icon in [
                    CursorIcon::Default,
                    CursorIcon::Pointer,
                    CursorIcon::Text,
                    CursorIcon::Wait,
                    CursorIcon::Grab,
                    CursorIcon::Grabbing,
                    CursorIcon::NwseResize,
                    CursorIcon::ZoomIn,
                    CursorIcon::ZoomOut,
                ] {
                    window.set_cursor(event_loop, icon);
                }
            }
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &mut EventLoop) {
        if let Some(started) = self.started_at
            && started.elapsed() > Duration::from_secs(3)
        {
            event_loop.exit();
        }
    }
}

#[test]
#[ignore]
fn cursor_shape_requests_flush_clean() {
    let _ = tracing_subscriber::fmt::try_init();

    let event_loop = EventLoop::<()>::new().expect("connect");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run_app");

    assert!(
        app.saw_configure,
        "expected configure handshake before exit"
    );
}
