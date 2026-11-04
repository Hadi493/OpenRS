# Phase 1.1: Catalog persistence on SQLite

## Overview

This report documents the completion of catalog persistence for both desktop and web targets. The catalog is now loaded from SQLite at startup (ADR 0003): the first run seeds a development sample into an empty database, and every later run reads what is stored.

## What was done

- **ADR 0003 accepted** — Catalog startup and the browser storage worker. Both shells load the catalog from SQLite at startup instead of from in-memory sample data.
- **`openrs-storage::load_or_seed_catalog`** — New shared function in `crates/openrs-storage/src/lib.rs` that seeds the database only when the products table is empty, then loads the catalog. The seed is all-or-nothing; a variant whose product is missing leaves the database unchanged.
- **Desktop** (`openrs-desktop`) — Uses `rusqlite` natively with WAL and `synchronous = FULL`. The first run seeds; later runs read the persisted catalog.
- **Web** (`openrs-web`) — Uses `sqlite-wasm-rs` + `sqlite-wasm-vfs` (sahpool OPFS) behind a dedicated worker (`crates/openrs-web-worker`). The worker opens the OPFS database, seeds if empty, reads the catalog, and posts a single `postMessage` back to the UI. The UI shows `Loading → Ready` or `Loading → Failed` based on the message.
- **Stable worker URL** — `Trunk.toml` sets `filehash = false`, so the worker loader script is always `./openrs-web-worker_loader.js` (Trunk hashes file names by default, which would make the URL unknowable from compiled Rust).
- **Cross-origin isolation** — `Trunk.toml` and the smoke test's static server both send `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`, which `SharedArrayBuffer` needs. A static host must send the same pair.
- **Desktop database location** — Overridable via `OPENRS_DATABASE` env var; otherwise `openrs.sqlite` in the platform data directory (`$XDG_DATA_HOME/openrs`, `~/Library/Application Support/OpenRS`, `%APPDATA%\OpenRS`).
- **UI states** — `OpenRsApp` starts in `Loading`, and `set_catalog` / `set_error` move it to `Ready` / `Failed`. The failed state shows the reason, because an unreadable database must not resemble a store with no products.

## Test results

- `cargo test --workspace`: all 19 catalog tests, 1 foundation, 5 startup, 13 guarantees, 4 native_file, 7 UI screen, 1 compile check pass.
- `wasm-pack test --headless --chrome crates/openrs-core`: 2 tests pass (catalog contract + foundation in WASM).
- `wasm-pack test --headless --chrome crates/openrs-storage`: 13 guarantees + 3 OPFS persistence tests pass.
- `scripts/smoke-desktop.sh`: `'OpenRS' window rendered` under Xvfb with wgpu rendering.
- `scripts/smoke-web.sh`: `OpenRS catalog ready: 4 products, 7 variants (Seeded)` in Chromium.

## Consequences

- Desktop and web now load the same catalog from SQLite. The first run writes the development sample; later runs read what was stored, so a change made by one session is what the next session sees.
- The web build has one more artifact (`openrs-web-worker`) and a worker round-trip at startup; the screen shows `"Loading catalog…"` while it happens.
- `filehash = false` is a deliberate tradeoff: stable names for the worker URL, at the cost of cache-busting. Revisit when the worker URL is resolved some other way.
- The protocol currently carries one message (catalog on success, error on failure). The checkout workflow will need request/response messages in the worker; that extension will reuse this framing and needs its own ADR if it grows beyond a small request set.
- `web_sys::Worker` has no timeout in this implementation: a worker that never answers leaves the loading state on screen. The smoke test fails on that, so CI would catch a regression, but an in-app watchdog is a possible follow-up.

## Open questions / follow-ups

- Watchdog timer for the worker (currently no timeout; a hung worker leaves loading state).
- Checkout workflow request/response messages in the worker (extension of the current one-shot protocol).
- Whether to persist the database path in `openrs.sqlite` across sessions on desktop (currently uses platform data directory).