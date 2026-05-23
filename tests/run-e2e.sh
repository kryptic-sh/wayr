#!/usr/bin/env bash
# Drive the wayr `#[ignore]`'d integration tests against a private
# headless sway compositor. Used by CI (#18) and runnable locally:
#
#   tests/run-e2e.sh
#
# Tests stay marked `#[ignore]` so the standard `cargo test` flow
# stays Wayland-free — only this script flips them on via
# `--include-ignored`.
#
# Exits non-zero if sway fails to start, the WAYLAND_DISPLAY
# socket doesn't appear within the timeout, or any test fails.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONF="${REPO_ROOT}/tests/e2e-sway.conf"

# Private XDG_RUNTIME_DIR so we don't collide with the user's
# real Wayland session if this runs locally on a live desktop.
# Sway picks a sequential `wayland-N` socket inside this dir; we
# read the actual name back after start (sway ignores
# WAYLAND_DISPLAY env on its own side).
RUNTIME_DIR="$(mktemp -d -t wayr-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"

# wlroots: pick the headless backend, no real input devices.
export WLR_BACKENDS=headless
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER_ALLOW_SOFTWARE=1
# Drop sway's WAYLAND_DISPLAY inheritance if any.
unset WAYLAND_DISPLAY

cleanup() {
    local rc=$?
    if [ -n "${SWAY_PID:-}" ]; then
        kill "$SWAY_PID" 2>/dev/null || true
        wait "$SWAY_PID" 2>/dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    exit "$rc"
}
trap cleanup EXIT INT TERM

SWAY_LOG="${SWAY_LOG:-/tmp/wayr-e2e-sway.log}"

echo "wayr e2e: starting headless sway (XDG_RUNTIME_DIR=$RUNTIME_DIR)"
sway --config "$CONF" >"$SWAY_LOG" 2>&1 &
SWAY_PID=$!

# Wait up to ~10s for the wayland-N socket to appear in our
# private runtime dir.
SOCKET_NAME=""
for _ in $(seq 1 50); do
    SOCKET_NAME="$(ls "$RUNTIME_DIR" 2>/dev/null | grep -E '^wayland-[0-9]+$' | head -1 || true)"
    if [ -n "$SOCKET_NAME" ]; then
        break
    fi
    if ! kill -0 "$SWAY_PID" 2>/dev/null; then
        echo "wayr e2e: sway exited before opening a socket" >&2
        echo "--- sway log tail ---" >&2
        tail -40 "$SWAY_LOG" >&2 || true
        exit 1
    fi
    sleep 0.2
done
if [ -z "$SOCKET_NAME" ]; then
    echo "wayr e2e: sway socket did not appear in $RUNTIME_DIR within 10s" >&2
    echo "--- sway log tail ---" >&2
    tail -40 "$SWAY_LOG" >&2 || true
    exit 1
fi
export WAYLAND_DISPLAY="$SOCKET_NAME"
echo "wayr e2e: socket up at \$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY; running tests"

# Run the full test suite including #[ignore]'d ones. Pass through
# any extra args (e.g. `tests/run-e2e.sh subsurface` to filter).
cd "$REPO_ROOT"
cargo test --all-features --no-fail-fast -- --include-ignored "$@"
