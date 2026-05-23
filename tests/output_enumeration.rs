//! Integration test: confirm `EventLoop::outputs()` reports the live
//! compositor outputs after the registry roundtrip + per-output `done`.
//!
//! Headless sway advertises one HEADLESS-1 output by default; we drive
//! the event loop just long enough to observe it.
//!
//! `#[ignore]` — needs live wayland. Wired into tests/run-e2e.sh.

use std::time::{Duration, Instant};

use wayr::{ApplicationHandler, EventLoop, OutputInfo, Size, SurfaceId, Toplevel, WindowEvent};

#[derive(Default)]
struct App {
    window: Option<Toplevel>,
    saw_configure: bool,
    started_at: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &mut EventLoop) {
        let toplevel = Toplevel::builder()
            .with_title("wayr output enum")
            .with_initial_size(Size::new(400, 300))
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
        if matches!(event, WindowEvent::RedrawRequested) {
            self.saw_configure = true;
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
fn outputs_enumerates_after_handshake() {
    let _ = tracing_subscriber::fmt::try_init();

    let event_loop = EventLoop::<()>::new().expect("connect");
    // Capture the outputs accessor before we move the event loop into
    // run_app. Snapshot doubles after run_app — once outputs.done has
    // fired they're populated.
    let mut app = App::default();
    let proxy_outputs_before: Vec<OutputInfo> = event_loop.outputs();
    // Even before drive, the wl_output proxies are bound; the per-
    // output state HashMap might be empty if no events arrived yet.
    // Just sanity-check the method runs.
    let _ = proxy_outputs_before;

    // Run the loop just long enough to receive the configure.
    let outputs_after_cell: std::rc::Rc<std::cell::RefCell<Vec<OutputInfo>>> = Default::default();
    {
        struct Wrapper {
            inner: App,
            outputs_cell: std::rc::Rc<std::cell::RefCell<Vec<OutputInfo>>>,
        }
        impl ApplicationHandler for Wrapper {
            fn resumed(&mut self, event_loop: &mut EventLoop) {
                self.inner.resumed(event_loop);
            }
            fn window_event(
                &mut self,
                event_loop: &mut EventLoop,
                sid: SurfaceId,
                ev: WindowEvent,
            ) {
                self.inner.window_event(event_loop, sid, ev);
                if self.inner.saw_configure {
                    *self.outputs_cell.borrow_mut() = event_loop.outputs();
                }
            }
            fn about_to_wait(&mut self, event_loop: &mut EventLoop) {
                self.inner.about_to_wait(event_loop);
            }
        }
        let mut wrapper = Wrapper {
            inner: app,
            outputs_cell: outputs_after_cell.clone(),
        };
        event_loop.run_app(&mut wrapper).expect("run_app");
        app = wrapper.inner;
    }

    assert!(app.saw_configure, "configure handshake should fire");
    let outputs = outputs_after_cell.borrow();
    assert!(
        !outputs.is_empty(),
        "headless sway advertises at least one wl_output"
    );
    let primary = &outputs[0];
    assert!(primary.scale >= 1, "scale defaults to 1 when unset");
    // sway with WLR_BACKENDS=headless advertises HEADLESS-1 with the
    // configured mode (1920x1080 per tests/e2e-sway.conf).
    assert!(
        primary.physical_size.width > 0 && primary.physical_size.height > 0,
        "expected non-zero output mode size, got {:?}",
        primary.physical_size
    );
}
