//! Subsurface integration test. `#[ignore]` because it requires a
//! live Wayland session.

#![cfg(feature = "subsurface")]

use std::time::{Duration, Instant};

use wayr::{
    ApplicationHandler, EventLoop, Position, Size, Subsurface, Surface, SurfaceId, Toplevel,
    WindowEvent,
};

#[derive(Default)]
struct App {
    parent: Option<Toplevel>,
    child: Option<Subsurface>,
    parent_resized: bool,
    started_at: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &mut EventLoop) {
        if self.parent.is_some() {
            return;
        }
        let parent = Toplevel::builder()
            .with_title("wayr subsurface test")
            .with_initial_size(Size::new(800, 600))
            .build(event_loop)
            .expect("build parent");
        let child = Subsurface::builder(&parent)
            .with_position(Position::new(20, 20))
            .with_size(Size::new(100, 100))
            .build(event_loop)
            .expect("build child");
        // Subsurface should have a stable id, distinct from parent.
        assert_ne!(parent.id(), child.id());
        self.parent = Some(parent);
        self.child = Some(child);
        self.started_at = Some(Instant::now());
    }

    fn window_event(
        &mut self,
        event_loop: &mut EventLoop,
        _surface_id: SurfaceId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::Resized(_)) {
            self.parent_resized = true;
        }
        if self.parent_resized {
            // Demonstrate set_position works after build.
            if let Some(child) = self.child.as_ref() {
                child.set_position(Position::new(40, 40));
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

    fn exiting(&mut self, _event_loop: &mut EventLoop) {
        // Drop child BEFORE parent.
        self.child.take();
        self.parent.take();
    }
}

#[test]
#[ignore]
fn subsurface_builds_and_repositions() {
    let _ = tracing_subscriber::fmt::try_init();
    let event_loop = EventLoop::<()>::new().expect("connect");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run_app");
    assert!(
        app.parent_resized,
        "expected at least one parent Resized event"
    );
}
