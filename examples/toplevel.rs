//! Minimal toplevel-window smoke test.
//!
//! Runs against the live `WAYLAND_DISPLAY` compositor. Opens an
//! 800×600 window titled "wayr toplevel", logs every received event
//! to stderr, and exits when the compositor sends `CloseRequested`.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG=info cargo run --example toplevel
//! ```

use wayr::{ApplicationHandler, EventLoop, Size, Surface, SurfaceId, Toplevel, WindowEvent};

#[derive(Default)]
struct App {
    window: Option<Toplevel>,
    redraws_received: u32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &mut EventLoop) {
        if self.window.is_some() {
            return;
        }
        let toplevel = Toplevel::builder()
            .with_title("wayr toplevel")
            .with_app_id("sh.kryptic.wayr.example.toplevel")
            .with_initial_size(Size::new(800, 600))
            .build(event_loop)
            .expect("build toplevel");
        tracing::info!(
            id = ?toplevel.id(),
            "toplevel constructed; waiting for first configure"
        );
        self.window = Some(toplevel);
    }

    fn window_event(
        &mut self,
        event_loop: &mut EventLoop,
        surface_id: SurfaceId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                tracing::info!(?surface_id, w = size.width, h = size.height, "Resized");
            }
            WindowEvent::ScaleFactorChanged {
                new_scale_factor, ..
            } => {
                tracing::info!(?surface_id, scale = new_scale_factor, "ScaleFactorChanged");
            }
            WindowEvent::RedrawRequested => {
                self.redraws_received += 1;
                tracing::info!(
                    ?surface_id,
                    count = self.redraws_received,
                    "RedrawRequested"
                );
            }
            WindowEvent::Focused => tracing::info!(?surface_id, "Focused"),
            WindowEvent::Unfocused => tracing::info!(?surface_id, "Unfocused"),
            WindowEvent::CloseRequested => {
                tracing::info!(?surface_id, "CloseRequested — exiting");
                event_loop.exit();
            }
            other => tracing::debug!(?surface_id, ?other, "window event"),
        }
    }

    fn exiting(&mut self, _event_loop: &mut EventLoop) {
        tracing::info!("event loop exiting");
    }
}

fn main() -> wayr::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let event_loop = EventLoop::<()>::new()?;
    event_loop.run_app(&mut App::default())
}
