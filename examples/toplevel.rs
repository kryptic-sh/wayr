//! Minimal toplevel-window smoke test.
//!
//! Runs against the live `WAYLAND_DISPLAY` compositor. Opens an
//! 800×600 window titled "wayr toplevel", paints a dark teal background
//! via wgpu, logs every event to stderr, and exits on `CloseRequested`.
//!
//! This is the canonical proof that wayr's `raw-window-handle` 0.6
//! integration is wired correctly — wgpu builds a `Surface` from the
//! handle, reconfigures on `Resized`, paints on `RedrawRequested`.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG=info cargo run --example toplevel
//! ```

use anyhow::Context;
use raw_window_handle::{DisplayHandle, HasDisplayHandle, HasWindowHandle, WindowHandle};
use wayr::{ApplicationHandler, EventLoop, Size, Surface, SurfaceId, Toplevel, WindowEvent};

/// wgpu rendering pieces that need to live as long as the surface they
/// were configured for.
struct Chrome {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl Chrome {
    fn new(window: &Toplevel, event_loop: &EventLoop) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });

        let display: DisplayHandle<'_> = event_loop.display_handle().context("display handle")?;
        let window_handle: WindowHandle<'_> = window.window_handle().context("window handle")?;

        // SAFETY: handles borrow `event_loop` + `window`; we extend the
        // lifetime via SurfaceTargetUnsafe because wgpu::Surface<'static>
        // can't carry the borrow. Chrome lives in the App struct
        // alongside the Toplevel and gets dropped first when App drops.
        let surface = unsafe {
            let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(display.as_raw()),
                raw_window_handle: window_handle.as_raw(),
            };
            instance.create_surface_unsafe(target)?
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .context("no wgpu adapter — install vulkan or GL drivers")?;
        tracing::info!(adapter = %adapter.get_info().name, "wgpu adapter");

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("wayr-example device"),
                ..Default::default()
            }))?;

        let initial = window.size();
        let initial_w = initial.width.max(1);
        let initial_h = initial.height.max(1);

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: initial_w,
            height: initial_h,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        Ok(Self {
            surface,
            device,
            queue,
            config,
        })
    }

    fn resize(&mut self, new_size: Size) {
        self.config.width = new_size.width.max(1);
        self.config.height = new_size.height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn paint(&self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                tracing::warn!(?other, "skipping frame");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wayr-example encoder"),
            });
        {
            let _rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wayr-example clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.10,
                            b: 0.13,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}

#[derive(Default)]
struct App {
    window: Option<Toplevel>,
    chrome: Option<Chrome>,
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
                // Lazy-init the wgpu chrome on the first Resized event
                // — the wgpu::Surface needs a non-zero size, which we
                // only get after the compositor's first configure.
                if self.chrome.is_none()
                    && let Some(window) = &self.window
                {
                    match Chrome::new(window, event_loop) {
                        Ok(chrome) => self.chrome = Some(chrome),
                        Err(err) => tracing::error!(error = %err, "chrome init failed"),
                    }
                }
                if let Some(chrome) = &mut self.chrome {
                    chrome.resize(size);
                }
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
                if let Some(chrome) = &self.chrome {
                    chrome.paint();
                }
            }
            WindowEvent::Focused => tracing::info!(?surface_id, "Focused"),
            WindowEvent::Unfocused => tracing::info!(?surface_id, "Unfocused"),
            WindowEvent::PointerEntered { position } => {
                tracing::info!(?surface_id, ?position, "PointerEntered");
            }
            WindowEvent::PointerLeft => tracing::info!(?surface_id, "PointerLeft"),
            WindowEvent::PointerMoved { position } => {
                tracing::debug!(?surface_id, ?position, "PointerMoved");
            }
            WindowEvent::PointerButton {
                button,
                state,
                modifiers,
            } => {
                tracing::info!(?surface_id, ?button, ?state, ?modifiers, "PointerButton");
            }
            WindowEvent::Scroll(scroll) => {
                tracing::info!(?surface_id, ?scroll, "Scroll");
            }
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
