//! Wayland connection + registry roundtrip + global binding.
//!
//! Internal module; the public API surface is constructed via
//! [`crate::EventLoop::new`].
//!
//! ## Why bind manually instead of using `smithay-client-toolkit`?
//!
//! sctk is a fine library, but it ships its own opinions about
//! window lifecycles + dispatch macros that conflict with wayr's
//! taxonomy (three explicit surface types, lifetime-bound subsurfaces).
//! The handful of globals we need is small enough to bind directly
//! without inheriting sctk's design assumptions.

use tracing::{debug, warn};
use wayland_client::globals::{Global, GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
use wayland_client::{Connection as WlConnection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;

use crate::error::{Error, Result};

/// Owning handle for the live Wayland connection + bound globals.
///
/// One per [`crate::EventLoop`]. Dropped on event-loop teardown; the
/// underlying socket closes when the last `WlConnection` Arc drops.
pub(crate) struct Connection {
    /// The raw wayland-client connection. Re-exposed to consumers via
    /// `EventLoop::raw_display_handle()` so wgpu can construct a
    /// surface from it.
    pub(crate) wl: WlConnection,

    /// Default event queue. `wayr` runs everything on this queue —
    /// the calloop adapter dispatches it from the event loop's pump.
    /// `EventQueue::handle()` is cloneable, so the per-surface code
    /// stores its own `QueueHandle<State>` rather than borrowing this
    /// field.
    pub(crate) queue: EventQueue<State>,

    /// All bound protocol globals.
    pub(crate) globals: Globals,
}

/// Bag of bound globals. Required ones are non-`Option`; optional
/// (feature-gated) globals are `Option`.
pub(crate) struct Globals {
    /// `wl_compositor` — creates `wl_surface`s. Always required.
    pub(crate) compositor: WlCompositor,

    /// `wl_subcompositor` — creates `wl_subsurface`s. Always bound;
    /// surface code behind the `subsurface` feature uses it. We bind
    /// unconditionally because the cost is one proxy + ~64 bytes, and
    /// we want consumer code that conditionally compiles subsurface
    /// support to get a useful error if the compositor missed
    /// advertising it.
    pub(crate) subcompositor: WlSubcompositor,

    /// `wl_shm` — shared memory for software-rendered surfaces +
    /// cursor themes. Always required (cursor fallback path).
    pub(crate) shm: WlShm,

    /// `wl_seat` — pointer/keyboard/touch input. v0.1 binds a single
    /// seat (`wl_seat@1`); multi-seat support is post-MVP.
    pub(crate) seat: WlSeat,

    /// `xdg_wm_base` — top-level windows. Always required (pings).
    pub(crate) xdg_wm_base: XdgWmBase,

    /// All `wl_output`s the compositor advertised. Multi-output state
    /// tracking lands in #13; v0.1 just keeps the bound proxies.
    pub(crate) outputs: Vec<WlOutput>,

    /// `zwlr_layer_shell_v1` — only when the `layer-shell` feature is
    /// on AND the compositor advertises it.
    #[cfg(feature = "layer-shell")]
    pub(crate) layer_shell: Option<
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
    >,
}

/// Dispatch state. Threaded through every wayland-client `Dispatch`
/// impl in this crate.
///
/// Kept deliberately small in v0.1 — fields land as the per-phase
/// tickets need them (e.g. per-surface state, audio queue, etc.).
pub(crate) struct State {
    /// Set by the `xdg_wm_base.ping` handler; `EventLoop::run_app`
    /// reads + clears it each iteration. Not used yet (no real loop
    /// hooked up); reserved for #7.
    pub(crate) _phantom: (),
}

impl Connection {
    /// Connect to `WAYLAND_DISPLAY`, perform the registry roundtrip,
    /// and bind every global wayr needs.
    pub(crate) fn connect_to_env() -> Result<Self> {
        let wl = WlConnection::connect_to_env().map_err(|err| {
            Error::NotWayland(format!(
                "wayland-client connection failed: {err} \
                 (is WAYLAND_DISPLAY set?)"
            ))
        })?;

        // `registry_queue_init` does the synchronous roundtrip on a
        // private queue and returns the global list — exactly the
        // pattern that was hard for buffr against winit's queue, but
        // easy here because wayr OWNS the connection.
        let (global_list, queue) = registry_queue_init::<State>(&wl).map_err(|err| {
            Error::Io(std::io::Error::other(format!(
                "registry queue init failed: {err}"
            )))
        })?;

        let qh = queue.handle();

        // Bind required globals via the global-list helper. Each call
        // returns the highest version we support that the compositor
        // advertised, or errors if the global is missing entirely.
        let compositor: WlCompositor = bind_required(&global_list, &qh, "wl_compositor", 5)?;
        let subcompositor: WlSubcompositor =
            bind_required(&global_list, &qh, "wl_subcompositor", 1)?;
        let shm: WlShm = bind_required(&global_list, &qh, "wl_shm", 1)?;
        let seat: WlSeat = bind_required(&global_list, &qh, "wl_seat", 7)?;
        let xdg_wm_base: XdgWmBase = bind_required(&global_list, &qh, "xdg_wm_base", 5)?;

        // Multi-output: bind all advertised wl_output globals. Each
        // surface tracks which outputs it touches via wl_surface.enter
        // / leave in Phase 4.
        let outputs: Vec<WlOutput> = global_list.contents().with_list(|globals| {
            globals
                .iter()
                .filter(|g: &&Global| g.interface == "wl_output")
                .map(|g: &Global| {
                    // wl_output is version 4 since Wayland 1.21;
                    // bind highest supported.
                    let version = g.version.min(4);
                    global_list
                        .registry()
                        .bind::<WlOutput, (), State>(g.name, version, &qh, ())
                })
                .collect()
        });
        debug!(count = outputs.len(), "bound wl_output globals");

        #[cfg(feature = "layer-shell")]
        let layer_shell: Option<
            wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
        > = bind_optional(&global_list, &qh, "zwlr_layer_shell_v1", 4);

        Ok(Connection {
            wl,
            queue,
            globals: Globals {
                compositor,
                subcompositor,
                shm,
                seat,
                xdg_wm_base,
                outputs,
                #[cfg(feature = "layer-shell")]
                layer_shell,
            },
        })
    }
}

/// Bind a required global. Logs a friendly error mentioning the
/// interface name if the compositor doesn't advertise it.
fn bind_required<I>(
    global_list: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<State>,
    name: &'static str,
    max_version: u32,
) -> Result<I>
where
    I: wayland_client::Proxy + 'static,
    State: Dispatch<I, ()>,
{
    global_list
        .bind::<I, State, ()>(qh, 1..=max_version, ())
        .map_err(|err| {
            warn!(
                interface = name,
                error = %err,
                "required global not advertised by compositor"
            );
            Error::MissingGlobal { name }
        })
}

/// Bind an optional global. Returns `None` (logged) if the compositor
/// doesn't advertise it.
fn bind_optional<I>(
    global_list: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<State>,
    name: &'static str,
    max_version: u32,
) -> Option<I>
where
    I: wayland_client::Proxy + 'static,
    State: Dispatch<I, ()>,
{
    match global_list.bind::<I, State, ()>(qh, 1..=max_version, ()) {
        Ok(proxy) => Some(proxy),
        Err(err) => {
            debug!(interface = name, error = %err, "optional global not present");
            None
        }
    }
}

// ── Dispatch impls (skeletons — events handled per phase) ────────────────────
//
// wayland-client requires Dispatch impls for every proxy we bind. v0.1
// drops most events on the floor; per-phase tickets add real handling.
// xdg_wm_base.ping → pong is the only one wired here because dropping
// it kills the connection (compositor decides the client is unresponsive).

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &WlConnection,
        _qh: &QueueHandle<Self>,
    ) {
        // wayland-client's `registry_queue_init` consumes the initial
        // global advertisements; subsequent dynamic adds/removes land
        // here. v0.1 ignores them (assume globals don't appear / vanish
        // mid-session). Multi-monitor hot-plug lands in #13.
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _: <WlCompositor as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on wl_compositor in the protocol; nothing to do.
    }
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSubcompositor,
        _: <WlSubcompositor as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShm, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _: <WlShm as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // wl_shm.format events advertise supported pixel formats.
        // v0.1 doesn't render via shm (wgpu's GPU path is preferred);
        // cursor-fallback shm code in #16 will start tracking formats.
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // Capabilities + name. #8 (pointer) + #10 (keyboard) attach
        // dispatch handlers that create wl_pointer / wl_keyboard
        // children based on the advertised capabilities.
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlOutput,
        _: <WlOutput as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // Geometry / mode / scale / name / description events.
        // Multi-output tracking lands in #13.
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _state: &mut Self,
        proxy: &XdgWmBase,
        event: <XdgWmBase as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_protocols::xdg::shell::client::xdg_wm_base::Event as XdgEvent;
        if let XdgEvent::Ping { serial } = event {
            // Required protocol response: failing to pong eventually
            // gets the client killed for unresponsiveness.
            proxy.pong(serial);
        }
    }
}

#[cfg(feature = "layer-shell")]
impl
    Dispatch<
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
        (),
    > for State
{
    fn event(
        _: &mut Self,
        _: &wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _: <wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &WlConnection,
        _: &QueueHandle<Self>,
    ) {
        // No events on zwlr_layer_shell_v1 itself; per-surface events
        // arrive via zwlr_layer_surface_v1 (handled in #11).
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live-compositor smoke test: connect, bind every required
    /// global, return cleanly. Marked `#[ignore]` because CI runners
    /// don't have a Wayland session — run with
    /// `cargo test -- --ignored` on a real desktop.
    #[test]
    #[ignore]
    fn connect_and_bind_globals() {
        // Permit tracing logs to surface in test output for debugging.
        let _ = tracing_subscriber::fmt::try_init();
        let conn = Connection::connect_to_env().expect("connect to env");
        // Required globals all present.
        let _ = &conn.globals.compositor;
        let _ = &conn.globals.subcompositor;
        let _ = &conn.globals.shm;
        let _ = &conn.globals.seat;
        let _ = &conn.globals.xdg_wm_base;
        // Outputs is a Vec — empty if no monitors connected, but the
        // bind step shouldn't fail.
        assert!(
            !conn.globals.outputs.is_empty(),
            "expected at least one wl_output on a real session"
        );
    }
}
