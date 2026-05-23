# wayr

Wayland-first windowing toolkit for Rust.

`wayr` is a minimal, opinionated alternative to winit aimed at
[kryptic-sh](https://github.com/kryptic-sh) apps —
[buffr](https://github.com/kryptic-sh/buffr),
[pikr](https://github.com/kryptic-sh/pikr), and future GUI work.

## What it is

- **Wayland only.** No X11, macOS, Windows, mobile, or web. No XWayland
  fallback. By construction.
- **First-class layer-shell + `wl_subsurface` embedding.** Both are
  built into the API rather than hidden behind raw FFI, because both
  are the reason wayr exists.
- **Fractional scaling + multi-output + text-input-v3 + clipboard.**
  Wayland-native through and through.
- **wgpu-friendly.** raw-window-handle 0.6 implementations, no version
  coupling to any specific GPU crate.

## What it isn't

- A drop-in winit replacement. The API shape mirrors winit's
  `ApplicationHandler` so consumer code ports mechanically, but wayr
  has no cross-platform pretensions.
- A general-purpose Wayland client library. Use
  [`wayland-client`](https://crates.io/crates/wayland-client) or
  [`smithay-client-toolkit`](https://crates.io/crates/smithay-client-toolkit)
  if you need raw protocol access.
- A clipboard library. Use
  [`hjkl-clipboard`](https://crates.io/crates/hjkl-clipboard) — it
  implements `wl_data_device` over a raw Wayland socket (orthogonal
  to wayr's own connection) and supports text + HTML + RTF +
  `image/png` MIME types, plus X11 / macOS / Windows / OSC52 fallback
  for free.
- Production-ready. Pre-alpha. Track [umbrella issue #1] for the v0.1
  MVP plan.

[umbrella issue #1]: https://github.com/kryptic-sh/wayr/issues/1

## Status

Phase 0 (scaffold + first toplevel window) in flight. See
[issues](https://github.com/kryptic-sh/wayr/issues) for the live punch
list.

## Why not fork winit?

Considered and rejected. Winit's cross-platform surface drags in X11,
macOS, Windows, and mobile code paths kryptic-sh apps will never use.
Floem's `floem-winit` fork was the leading candidate base, but its
restructured per-platform subcrate layout and calver versioning make
rebases against upstream painful. Building a focused Wayland-only
toolkit from scratch — ~4-6k LOC of glue on top of `wayland-client` +
`wayland-protocols` + `xkbcommon` — is comparable scope to a fork
maintained over a year, with the bonus of an API shaped around the
embedder use cases (subsurface, layer-shell) we actually need.

## License

MIT. See [LICENSE](./LICENSE).
