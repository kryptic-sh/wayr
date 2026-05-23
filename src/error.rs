//! Public error type.

use std::fmt;

/// Errors surfaced by `wayr` to consumers.
///
/// The variants intentionally avoid leaking concrete `wayland-client` /
/// `calloop` / `xkbcommon` error types — those crates are implementation
/// details. Match on variants here, or `Display`-format for human output.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// No `WAYLAND_DISPLAY` is set, or the socket couldn't be opened.
    ///
    /// Almost always means the user is running an X11 session or no
    /// display server at all. `wayr` does not fall back to X11 — see
    /// the crate-level docs for the rationale.
    NotWayland(String),

    /// The compositor doesn't advertise a global `wayr` needs.
    ///
    /// `name` is the protocol interface (`"xdg_wm_base"`,
    /// `"wl_compositor"`, etc.). For optional protocols (layer-shell,
    /// text-input-v3, cursor-shape) this is only emitted when the
    /// consumer enabled the corresponding feature and attempted to use
    /// the protocol.
    MissingGlobal {
        /// Interface name of the missing global.
        name: &'static str,
    },

    /// I/O failure on the Wayland socket. The connection is likely no
    /// longer usable; consumer should shut down.
    Io(std::io::Error),

    /// A Wayland protocol error reported by the server. `code` is the
    /// `wl_display.error` code; `interface` is the object's interface
    /// name; `message` is the server-supplied description.
    Protocol {
        /// Interface name of the offending object.
        interface: &'static str,
        /// Server-defined error code.
        code: u32,
        /// Server-supplied human-readable message.
        message: String,
    },

    /// xkbcommon failed to parse the keymap the compositor sent.
    Keymap(String),

    /// The application asked for an operation that's not applicable to
    /// the current surface kind (e.g. `set_anchor` on a toplevel).
    InvalidOperation(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotWayland(msg) => write!(f, "not a wayland session: {msg}"),
            Error::MissingGlobal { name } => {
                write!(f, "compositor does not advertise the {name} global")
            }
            Error::Io(err) => write!(f, "wayland socket I/O: {err}"),
            Error::Protocol {
                interface,
                code,
                message,
            } => write!(
                f,
                "wayland protocol error on {interface} (code {code}): {message}"
            ),
            Error::Keymap(msg) => write!(f, "xkbcommon keymap: {msg}"),
            Error::InvalidOperation(op) => write!(f, "invalid operation: {op}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

/// Convenience alias used throughout the public API.
pub type Result<T> = std::result::Result<T, Error>;
