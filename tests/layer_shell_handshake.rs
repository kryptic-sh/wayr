//! Layer-shell integration test. `#[ignore]` because it requires
//! a live compositor that advertises `zwlr_layer_shell_v1`.

#![cfg(feature = "layer-shell")]

use std::time::{Duration, Instant};

use wayr::{
    Anchor, ApplicationHandler, EventLoop, KeyboardInteractivity, Layer, LayerSurface, Size,
    SurfaceId, WindowEvent,
};

#[derive(Default)]
struct App {
    bar: Option<LayerSurface>,
    saw_resized: Option<Size>,
    saw_redraw: bool,
    started_at: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &mut EventLoop) {
        if self.bar.is_some() {
            return;
        }
        let bar = LayerSurface::builder()
            .with_layer(Layer::Top)
            .with_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .with_size(Size::new(0, 32))
            .with_exclusive_zone(32)
            .with_keyboard_interactivity(KeyboardInteractivity::None)
            .build(event_loop)
            .expect("build layer surface");
        self.bar = Some(bar);
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
            WindowEvent::RedrawRequested => self.saw_redraw = true,
            _ => {}
        }
        if self.saw_resized.is_some() && self.saw_redraw {
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
fn layer_surface_configure_handshake_completes() {
    let _ = tracing_subscriber::fmt::try_init();

    let event_loop = EventLoop::<()>::new().expect("connect");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run_app");

    let size = app
        .saw_resized
        .expect("expected Resized after layer-shell configure");
    assert!(size.height > 0, "expected non-zero configured height");
    assert!(
        app.saw_redraw,
        "expected RedrawRequested after configure ack"
    );
}
