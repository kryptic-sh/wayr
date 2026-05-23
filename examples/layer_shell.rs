//! Layer-shell smoke test — anchored top panel.
//!
//! Requires the `layer-shell` feature. Compositor must advertise
//! `zwlr_layer_shell_v1` (sway, Hyprland, KDE 5.27+, river, etc.;
//! Mutter does not).
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG=info cargo run --example layer_shell --features layer-shell
//! ```

#[cfg(feature = "layer-shell")]
fn main() -> wayr::Result<()> {
    use wayr::{
        Anchor, ApplicationHandler, EventLoop, KeyboardInteractivity, Layer, LayerSurface, Size,
        SurfaceId, WindowEvent,
    };

    #[derive(Default)]
    struct App {
        bar: Option<LayerSurface>,
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
                .with_namespace("wayr-example-bar")
                .build(event_loop)
                .expect("build layer surface");
            tracing::info!("layer-shell panel ready");
            self.bar = Some(bar);
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

#[cfg(not(feature = "layer-shell"))]
fn main() {
    eprintln!("Compile with --features layer-shell to run this example.");
}
