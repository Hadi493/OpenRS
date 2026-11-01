#!/usr/bin/env bash
# Headless smoke test for the native OpenRS desktop shell.
#
# Why this exists
# ---------------
# `cargo test` covers the shared core and the shared egui UI through
# `egui_kittest`, but it never proves that the native shell can actually open a
# window and render through `wgpu`. This script closes that gap in CI and on
# developer machines without a display.
#
# What it does
# ------------
# 1. Starts a virtual X server (`Xvfb`).
# 2. Launches the desktop binary with the OpenGL `wgpu` backend. Mesa's
#    `llvmpipe` software rasterizer is used when no GPU is present, which is the
#    normal case in CI.
# 3. Waits for the window to map, captures a screenshot, and fails if the
#    process exited early.
#
# Requirements: `Xvfb`, `xwininfo`, `import` (ImageMagick).
#
# Usage:
#   scripts/smoke-desktop.sh [output.png]
#
# Environment:
#   BIN          - binary to run (default: target/debug/openrs-desktop)
#   DISPLAY_NUM  - virtual display number (default: :99)
#   WGPU_BACKEND - wgpu backend (default: gl)
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISPLAY_NUM="${DISPLAY_NUM:-:99}"
OUT="${1:-${TMPDIR:-/tmp}/openrs-desktop.png}"
BIN="${BIN:-$ROOT_DIR/target/debug/openrs-desktop}"
LOG="${TMPDIR:-/tmp}/openrs-desktop.log"
WINDOW_TITLE="OpenRS"
WGPU_BACKEND="${WGPU_BACKEND:-gl}"

if [[ ! -x "$BIN" ]]; then
    echo "error: $BIN is missing; run 'cargo build -p openrs-desktop' first" >&2
    exit 1
fi

cleanup() {
    [[ -n "${APP_PID:-}" ]] && kill -9 "$APP_PID" 2>/dev/null
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null
    wait 2>/dev/null || true
}
trap cleanup EXIT

rm -f "$OUT" "$LOG"
pkill -f "Xvfb $DISPLAY_NUM" 2>/dev/null
sleep 1

Xvfb "$DISPLAY_NUM" -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
XVFB_PID=$!
sleep 2

# `WAYLAND_DISPLAY` must be cleared: winit prefers Wayland over the virtual X
# server and would then create the window on the host compositor instead.
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
    DISPLAY="$DISPLAY_NUM" \
    WINIT_UNIX_BACKEND=x11 \
    WGPU_BACKEND="$WGPU_BACKEND" \
    "$BIN" >"$LOG" 2>&1 &
APP_PID=$!
sleep 10

if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "FAIL: the desktop app exited before the window could be captured" >&2
    cat "$LOG" >&2
    exit 1
fi

if ! DISPLAY="$DISPLAY_NUM" xwininfo -root -tree | grep -q "\"$WINDOW_TITLE\""; then
    echo "FAIL: no window titled '$WINDOW_TITLE' was mapped" >&2
    cat "$LOG" >&2
    exit 1
fi

if ! DISPLAY="$DISPLAY_NUM" import -window root "$OUT" 2>/dev/null; then
    echo "FAIL: could not capture a screenshot of the virtual display" >&2
    exit 1
fi

echo "PASS: '$WINDOW_TITLE' window rendered; screenshot written to $OUT"
