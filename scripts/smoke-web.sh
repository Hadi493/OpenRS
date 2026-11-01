#!/usr/bin/env bash
# Headless smoke test for the WebAssembly OpenRS web build.
#
# Why this exists
# ---------------
# `wasm-pack test` proves the shared *core* runs in a browser, but it does not
# prove that the `openrs-web` shell boots: that the wasm module loads, that the
# canvas is found, and that `eframe::WebRunner` starts without panicking.
#
# What it does
# ------------
# 1. Builds the web app with Trunk (unless `SKIP_BUILD=1`).
# 2. Serves `dist/` on a loopback port.
# 3. Loads the page in headless Chromium with the SwiftShader software
#    renderer, so WebGL works without a GPU.
# 4. Asserts that the canvas exists and that the `#loading_text` placeholder was
#    removed, which only happens after `WebRunner::start` succeeds.
# 5. Captures a screenshot for manual inspection.
#
# Requirements: `trunk`, `wasm-bindgen` CLI, Chromium or Google Chrome,
# `curl`, and `python3` (used as a throwaway static file server).
#
# Usage:
#   scripts/smoke-web.sh [screenshot.png]
#
# Environment:
#   SKIP_BUILD - set to 1 to reuse an existing `dist/`
#   BROWSER    - browser binary (default: first of chromium, google-chrome,
#                google-chrome-stable found on PATH)
#   PORT       - loopback port for the static server (default: 8123)
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="$ROOT_DIR/crates/openrs-web"
OUT="${1:-${TMPDIR:-/tmp}/openrs-web.png}"
PORT="${PORT:-8123}"
URL="http://127.0.0.1:$PORT/index.html"

BROWSER="${BROWSER:-}"
if [[ -z "$BROWSER" ]]; then
    for candidate in chromium chromium-browser google-chrome google-chrome-stable; do
        if command -v "$candidate" >/dev/null 2>&1; then
            BROWSER="$candidate"
            break
        fi
    done
fi
if [[ -z "$BROWSER" ]]; then
    echo "error: no Chromium/Chrome binary found; set BROWSER" >&2
    exit 1
fi

cleanup() {
    [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null
    wait 2>/dev/null || true
}
trap cleanup EXIT

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
    echo "building web app with trunk..."
    (cd "$WEB_DIR" && trunk build --release=true) || {
        echo "FAIL: trunk build failed" >&2
        exit 1
    }
fi

if [[ ! -f "$WEB_DIR/dist/index.html" ]]; then
    echo "error: $WEB_DIR/dist/index.html is missing; run trunk build first" >&2
    exit 1
fi

(cd "$WEB_DIR/dist" && python3 -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1) &
SERVER_PID=$!
sleep 2

if ! curl -fsS -o /dev/null "$URL"; then
    echo "FAIL: static server did not serve $URL" >&2
    exit 1
fi

CHROME_ARGS=(
    --headless=new
    --no-sandbox
    --disable-dev-shm-usage
    --hide-scrollbars
    --window-size=1024,700
    --use-angle=swiftshader
    --enable-unsafe-swiftshader
    --virtual-time-budget=20000
)

DOM="$("$BROWSER" "${CHROME_ARGS[@]}" --dump-dom "$URL" 2>/dev/null)"

if ! grep -q 'id="the_canvas_id"' <<<"$DOM"; then
    echo "FAIL: the canvas element was not found in the rendered DOM" >&2
    exit 1
fi

# `openrs-web` removes this element only after `WebRunner::start` resolves
# successfully, so its absence is the startup-success signal.
if grep -q 'id="loading_text"' <<<"$DOM"; then
    echo "FAIL: the app did not start; '#loading_text' is still present" >&2
    exit 1
fi

"$BROWSER" "${CHROME_ARGS[@]}" --screenshot="$OUT" "$URL" >/dev/null 2>&1 || {
    echo "FAIL: could not capture a screenshot" >&2
    exit 1
}

echo "PASS: web app started in $BROWSER; screenshot written to $OUT"
