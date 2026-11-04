#!/usr/bin/env bash
# Headless smoke test for the WebAssembly OpenRS web build.
#
# Why this exists
# ---------------
# `wasm-pack test` proves the shared *core* and the *storage* layer run in a
# browser, but neither proves that the `openrs-web` shell boots: that the wasm
# module loads, the canvas is found, `eframe::WebRunner` starts, and the storage
# worker hands a catalog to the UI.
#
# What it does
# ------------
# 1. Builds the web app with Trunk (unless `SKIP_BUILD=1`).
# 2. Serves `dist/` on a loopback port with the COOP/COEP headers the OPFS
#    storage worker needs (the same pair `Trunk.toml` sets for `trunk serve`).
# 3. Starts headless Chromium with the SwiftShader software renderer and a
#    remote-debugging port, then drives it with `scripts/web-cdp.mjs`.
# 4. Asserts the canvas exists, `#loading_text` is gone (the signal that
#    `WebRunner::start` resolved), and the console reports the catalog the
#    storage worker loaded.
# 5. Captures a screenshot for manual inspection.
#
# Requirements: `trunk`, `wasm-bindgen` CLI, Chromium or Google Chrome, `node`
# (for the DevTools protocol helper), `curl`, and `python3` (used as a throwaway
# static file server).
#
# Usage:
#   scripts/smoke-web.sh [screenshot.png]
#
# Environment:
#   SKIP_BUILD           - set to 1 to reuse an existing `dist/`
#   BROWSER              - browser binary (default: first of chromium,
#                          chromium-browser, google-chrome, google-chrome-stable
#                          found on PATH)
#   PORT                 - loopback port for the static server (default: 8123)
#   CDP_PORT             - remote debugging port (default: 9222)
#   STARTUP_BUDGET_MS    - how long to wait for the app to start (default: 60000)
#   KEEP_CONSOLE         - set to 1 to keep the captured console output file
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="$ROOT_DIR/crates/openrs-web"
OUT="${1:-${TMPDIR:-/tmp}/openrs-web.png}"
PORT="${PORT:-8123}"
CDP_PORT="${CDP_PORT:-9222}"
URL="http://127.0.0.1:$PORT/index.html"
STARTUP_BUDGET_MS="${STARTUP_BUDGET_MS:-60000}"

# Override to keep the captured browser console output after the run.
CONSOLE_LOG="${CONSOLE_LOG:-$(mktemp)}"
PROFILE_DIR="$(mktemp -d)"

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

if ! command -v node >/dev/null 2>&1; then
    echo "error: node is required to drive Chromium over the DevTools protocol" >&2
    exit 1
fi

cleanup() {
    [[ -n "${CHROME_PID:-}" ]] && kill "$CHROME_PID" 2>/dev/null
    [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null
    rm -rf "$PROFILE_DIR" 2>/dev/null || true
    [[ -z "${KEEP_CONSOLE:-}" ]] && rm -f "$CONSOLE_LOG"
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

# A plain `python3 -m http.server` does not send COOP/COEP, and without them
# `SharedArrayBuffer` is unavailable to the storage worker: the page would start
# and then show the catalog error state instead of a catalog.
(cd "$WEB_DIR/dist" && python3 - "$PORT" >/dev/null 2>&1 <<'PY'
import http.server
import socketserver
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()


port = int(sys.argv[1])
socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(("127.0.0.1", port), Handler) as server:
    server.serve_forever()
PY
) &
SERVER_PID=$!
sleep 2

if ! curl -fsS -o /dev/null "$URL"; then
    echo "FAIL: static server did not serve $URL" >&2
    exit 1
fi

# The 32 MB wasm bundle plus the worker's OPFS migration need a real wait under
# software rendering; `web-cdp.mjs` polls until the app has actually started.
CHROME_ARGS=(
    --headless=new
    --no-sandbox
    --disable-dev-shm-usage
    --hide-scrollbars
    --window-size=1024,700
    --use-angle=swiftshader
    --enable-unsafe-swiftshader
    --remote-debugging-port="$CDP_PORT"
    --user-data-dir="$PROFILE_DIR"
)

"$BROWSER" "${CHROME_ARGS[@]}" about:blank >"$CONSOLE_LOG" 2>&1 &
CHROME_PID=$!

for _ in $(seq 1 40); do
    if curl -fsS -o /dev/null "http://127.0.0.1:$CDP_PORT/json/version" 2>/dev/null; then
        break
    fi
    sleep 0.5
done

if ! curl -fsS -o /dev/null "http://127.0.0.1:$CDP_PORT/json/version" 2>/dev/null; then
    echo "FAIL: Chromium did not open its debugging port on $CDP_PORT" >&2
    exit 1
fi

CDP_OUT="$(mktemp)"
node "$ROOT_DIR/scripts/web-cdp.mjs" \
    "http://127.0.0.1:$CDP_PORT" \
    "$URL" \
    "$OUT" \
    "$STARTUP_BUDGET_MS" | tee "$CDP_OUT"
CDP_STATUS=${PIPESTATUS[0]}

if [[ "$CDP_STATUS" -ne 0 ]]; then
    echo "FAIL: the app did not start (web-cdp.mjs exited $CDP_STATUS)" >&2
    cat "$CDP_OUT" >&2
    rm -f "$CDP_OUT"
    exit 1
fi

# `openrs-web-worker` logs this only after it has opened the OPFS database,
# migrated it, seeded it if needed, and read the catalog back. Startup alone
# would not prove that the storage path works in a browser.
if ! grep -q 'OpenRS catalog ready: 4 products' "$CDP_OUT"; then
    echo "FAIL: the storage worker did not report a loaded catalog" >&2
    rm -f "$CDP_OUT"
    exit 1
fi

if [[ ! -s "$OUT" ]]; then
    echo "FAIL: no screenshot was written to $OUT" >&2
    rm -f "$CDP_OUT"
    exit 1
fi

rm -f "$CDP_OUT"
echo "PASS: web app started and loaded its catalog in $BROWSER; screenshot written to $OUT"
