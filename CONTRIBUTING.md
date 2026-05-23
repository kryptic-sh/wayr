# Contributing to wayr

`wayr` is a Wayland-first windowing toolkit for Rust. This file overrides the
[org-wide CONTRIBUTING.md](https://github.com/kryptic-sh/.github/blob/main/.github/CONTRIBUTING.md)
with wayr-specific dev setup, scope, and release flow. For everything not
covered here (Conventional Commits, MSRV stance, snapshot policy, generic Rust
PR checklist), defer to the org-wide guide.

## Scope (read before opening a PR)

- **Linux + Wayland only.** No X11, macOS, Windows, mobile, web. Ever.
- **Library, not application.** The crate ships protocol primitives + an event
  loop; rendering is the consumer's job via `raw-window-handle` 0.6.
- **Minimal opinion.** Only the surface area buffr / pikr / future kryptic-sh
  apps actually need. If a feature would benefit a third party but no kryptic-sh
  app needs it, file an issue first.
- **No upstream feature parity.** This is not a winit replacement for the
  general ecosystem — it's the Wayland-only slice we ship.

## Dev setup

```bash
git clone git@github.com:kryptic-sh/wayr.git
cd wayr
# System deps (Debian / Ubuntu):
sudo apt-get install -y libwayland-dev libxkbcommon-dev pkg-config
cargo test --all-features
```

Arch:

```bash
sudo pacman -S wayland wayland-protocols libxkbcommon pkgconf
```

## Before pushing

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check
cargo test --workspace --all-features --no-fail-fast
```

CI runs the same on every PR. New public API needs rustdoc.

## Features

Every protocol module beyond the core gets a Cargo feature flag. The minimum
core (default) is `wl_compositor` + `xdg_shell` + `wl_seat` (pointer +
keyboard). Opt-in:

| Feature            | Brings in                                            |
| ------------------ | ---------------------------------------------------- |
| `layer-shell`      | `zwlr_layer_shell_v1` (pikr-style anchored surfaces) |
| `subsurface`       | `wl_subsurface` (buffr-style WPE WebKit embedding)   |
| `text-input`       | `zwp_text_input_v3` (IME composition)                |
| `cursor-shape`     | `wp_cursor_shape_manager_v1`                         |
| `fractional-scale` | `wp_fractional_scale_manager_v1` + `wp_viewporter`   |

Avoid defaulting protocols on. If you add a new feature, document its globals
and event-loop side effects in the relevant `src/<feature>.rs` module-level
doc.

Clipboard is intentionally out of scope — consumers use
[`hjkl-clipboard`](https://github.com/kryptic-sh/hjkl) which has its own Wayland
data-device implementation with image / HTML / RTF + X11 / macOS / Windows
fallbacks.

## End-to-end tests

Integration tests are marked `#[ignore]` so the standard `cargo test` flow stays
Wayland-free. The full suite runs against a private headless sway:

```bash
sudo apt-get install -y sway mesa-vulkan-drivers
tests/run-e2e.sh                 # all tests
tests/run-e2e.sh subsurface      # filter to one test name
```

The script spawns sway with `WLR_BACKENDS=headless` in a private
`XDG_RUNTIME_DIR`, polls for the `wayland-N` socket, and runs
`cargo test -- --include-ignored`.

CI runs the same script (`e2e` job in `.github/workflows/ci.yml`). New
integration tests go in `tests/<name>.rs`, marked `#[ignore]`, with a comment
explaining what protocol path they cover.

## Releases (BCTP)

wayr cuts releases through the org-standard **BCTP** flow:

1. **B**ump `version` in `Cargo.toml` (semver). Regenerate `Cargo.lock` with
   `cargo build`. Update `CHANGELOG.md` — move entries under `## [Unreleased]`
   to a new `## [X.Y.Z] - YYYY-MM-DD` heading; add the matching
   `[X.Y.Z]: …/releases/tag/vX.Y.Z` reference-link definition.
2. **C**ommit with `chore: bump version`. Stage `Cargo.toml`, `Cargo.lock`,
   `CHANGELOG.md`.
3. **T**ag the commit as `vX.Y.Z`.
4. **P**ush commit + tag. The tag triggers the `publish-crates` job in `ci.yml`,
   which idempotently publishes to crates.io.

Patch = bug-fix only. Minor = additive public API. Major = breaking change.

To yank a broken release:

```bash
cargo yank --version X.Y.Z -p wayr
```

Document the reason under `### Yanked` in `CHANGELOG.md`.

## Reporting bugs / requesting features

Use GitHub issues. For security reports, the org policy in
[`SECURITY.md`](https://github.com/kryptic-sh/.github/blob/main/.github/SECURITY.md)
applies — email `mxaddict@kryptic.sh`, do not open a public issue.

## Code of Conduct

This project follows the
[Contributor Covenant](https://github.com/kryptic-sh/.github/blob/main/.github/CODE_OF_CONDUCT.md).
