//! Subsurface smoke test — toplevel with a child subsurface.
//!
//! Demonstrates the wl_subsurface API buffr's WPE WebKit backend
//! will use to embed the browser engine inside buffr's chrome.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG=info cargo run --example subsurface --features subsurface
//! ```

#[cfg(feature = "subsurface")]
fn main() -> wayr::Result<()> {
    use wayr::{
        ApplicationHandler, EventLoop, Position, Size, Subsurface, Surface, SurfaceId, Toplevel,
        WindowEvent,
    };

    #[derive(Default)]
    struct App {
        parent: Option<Toplevel>,
        child: Option<Subsurface>,
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &mut EventLoop) {
            if self.parent.is_some() {
                return;
            }
            let parent = Toplevel::builder()
                .with_title("wayr subsurface — parent")
                .with_app_id("sh.kryptic.wayr.example.subsurface")
                .with_initial_size(Size::new(800, 600))
                .build(event_loop)
                .expect("build parent");
            tracing::info!(parent_id = ?parent.id(), "parent toplevel built");

            let child = Subsurface::builder(&parent)
                .with_position(Position::new(50, 50))
                .with_size(Size::new(300, 200))
                .build(event_loop)
                .expect("build child subsurface");
            tracing::info!(child_id = ?child.id(), "child subsurface built");

            self.parent = Some(parent);
            self.child = Some(child);
        }

        fn window_event(
            &mut self,
            event_loop: &mut EventLoop,
            surface_id: SurfaceId,
            event: WindowEvent,
        ) {
            tracing::info!(?surface_id, ?event, "window event");
            if matches!(event, WindowEvent::CloseRequested) {
                event_loop.exit();
            }
        }

        fn exiting(&mut self, _event_loop: &mut EventLoop) {
            // Drop child BEFORE parent (protocol-correct order).
            self.child.take();
            self.parent.take();
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let event_loop = EventLoop::<()>::new()?;
    event_loop.run_app(&mut App::default())
}

#[cfg(not(feature = "subsurface"))]
fn main() {
    eprintln!("Compile with --features subsurface to run this example.");
}
