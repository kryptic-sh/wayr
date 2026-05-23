//! Integration test: build a toplevel + drive the event loop until
//! the compositor's configure handshake completes.
//!
//! `#[ignore]` because this needs a live Wayland session. Wired into
//! the headless-sway CI infrastructure (#18) once that lands.

use std::time::{Duration, Instant};

use wayr::{ApplicationHandler, EventLoop, Size, SurfaceId, Toplevel, WindowEvent};

#[derive(Default)]
struct App {
    window: Option<Toplevel>,
    saw_resized: Option<Size>,
    saw_scale: Option<f64>,
    saw_redraw: bool,
    started_at: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &mut EventLoop) {
        if self.window.is_some() {
            return;
        }
        let toplevel = Toplevel::builder()
            .with_title("wayr handshake test")
            .with_initial_size(Size::new(800, 600))
            .build(event_loop)
            .expect("build toplevel");
        self.window = Some(toplevel);
        self.started_at = Some(Instant::now());
    }

    fn window_event(
        &mut self,
        event_loop: &mut EventLoop,
        _surface_id: SurfaceId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => self.saw_resized = Some(size),
            WindowEvent::ScaleFactorChanged {
                new_scale_factor, ..
            } => self.saw_scale = Some(new_scale_factor),
            WindowEvent::RedrawRequested => self.saw_redraw = true,
            _ => {}
        }
        if self.saw_resized.is_some() && self.saw_scale.is_some() && self.saw_redraw {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &mut EventLoop) {
        // Bail after 3 seconds in case the compositor never resolves
        // the configure (would only happen under broken sessions).
        if let Some(started) = self.started_at
            && started.elapsed() > Duration::from_secs(3)
        {
            event_loop.exit();
        }
    }
}

#[test]
#[ignore]
fn toplevel_configure_handshake_completes() {
    let _ = tracing_subscriber::fmt::try_init();

    let event_loop = EventLoop::<()>::new().expect("connect");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run_app");

    let size = app
        .saw_resized
        .expect("expected at least one Resized event from compositor configure");
    assert!(size.width > 0, "expected non-zero width from configure");
    assert!(size.height > 0, "expected non-zero height from configure");

    let scale = app
        .saw_scale
        .expect("expected at least one ScaleFactorChanged event");
    assert!(scale > 0.0, "scale must be positive");

    assert!(
        app.saw_redraw,
        "expected at least one RedrawRequested event after configure ack"
    );
}
