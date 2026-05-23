//! Minimal keyboard-input smoke test.
//!
//! Runs against the live `WAYLAND_DISPLAY` compositor. Opens a small
//! window, logs every key down with the translated text (for typeable
//! keys) and the key name (for non-typeable keys like F1 / Return /
//! arrows). Press `q` (with no modifiers) to exit.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG=info cargo run --example typewriter
//! ```

use wayr::{
    ApplicationHandler, EventLoop, KeyCode, KeyState, Size, SurfaceId, Toplevel, WindowEvent,
};

#[derive(Default)]
struct App {
    window: Option<Toplevel>,
    buffer: String,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &mut EventLoop) {
        if self.window.is_some() {
            return;
        }
        let toplevel = Toplevel::builder()
            .with_title("wayr typewriter")
            .with_app_id("sh.kryptic.wayr.example.typewriter")
            .with_initial_size(Size::new(600, 200))
            .build(event_loop)
            .expect("build toplevel");
        tracing::info!("typewriter ready — press q to exit");
        self.window = Some(toplevel);
    }

    fn window_event(
        &mut self,
        event_loop: &mut EventLoop,
        surface_id: SurfaceId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Focused => tracing::info!(?surface_id, "Focused"),
            WindowEvent::Unfocused => tracing::info!(?surface_id, "Unfocused"),
            WindowEvent::Key(key) => {
                if !matches!(key.state, KeyState::Pressed) {
                    return;
                }
                let key_name = match &key.key_code {
                    KeyCode::Named(s) => s.as_str(),
                    KeyCode::Sym(_) => "<sym>",
                    _ => "<unknown>",
                };
                tracing::info!(
                    ?key.modifiers,
                    key = key_name,
                    text = key.text.as_deref().unwrap_or(""),
                    "Key"
                );
                if let Some(text) = key.text.as_deref() {
                    self.buffer.push_str(text);
                    tracing::info!(buffer = self.buffer, "buffer");
                }
                // Quit on plain `q` (no Ctrl / Alt / Super).
                if key.text.as_deref() == Some("q")
                    && !key.modifiers.ctrl
                    && !key.modifiers.alt
                    && !key.modifiers.logo
                {
                    tracing::info!("q pressed — exiting");
                    event_loop.exit();
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &mut EventLoop) {
        tracing::info!(buffer = self.buffer, "final buffer");
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
