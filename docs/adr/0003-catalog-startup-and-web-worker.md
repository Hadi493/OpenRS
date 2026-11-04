# ADR 0003: Catalog startup and the browser storage worker

- **Status:** Accepted
- **Date:** 2026-11-02
- **Supersedes:** nothing; extends ADR 0002 (storage engine)
- **Deciders:** project owner (seeding policy), contributor (implementation)

## Context

ADR 0002 put SQLite on both targets and made the guarantee suite run natively
and in a browser. Phase 1 then shipped the catalog screen from shared in-memory
sample data, and its report recorded the blocker: *"loading it from
`openrs-storage` requires a synchronous read on desktop and the worker adapter on
web, so both targets can adopt it at the same time."*

Three facts constrain the answer:

1. OPFS sync access handles only exist inside a dedicated worker, so a browser
   SQLite database cannot live on the UI thread at all (ADR 0002, condition 3).
2. The project forbids handwritten JavaScript application code and any webview
   shell (`AGENTS.md`), so the worker must be Rust compiled to WebAssembly and
   started by the toolchain's own generated glue.
3. Desktop and web must show the same catalog from the same rules, and a storage
   failure must not look like an empty catalog (`UI-UX.md`).

## Options considered

### A. Keep the shared in-memory sample (status quo)

Both shells start from `Catalog::sample()` and never touch storage.

- *Pro:* simplest; identical on both targets.
- *Con:* the catalog is rebuilt on every start, stock changes made by a checkout
  would be lost, and `openrs-storage` would be dead code in the application.
- *Rejected:* it does not satisfy "both shells load the catalog at startup".

### B. Read the catalog on the UI thread in the browser

Open the database on the main thread (in-memory VFS, or serialize the file to
IndexedDB around each read).

- *Pro:* no worker, no message protocol, synchronous code path shared with
  desktop.
- *Con:* no OPFS sync handles on the main thread, so durability would have to be
  faked by writing whole database images asynchronously; a committed sale could
  be lost in a crash window, which contradicts ADR 0002's `synchronous = FULL`
  promise.
- *Rejected:* it trades the storage guarantees for convenience.

### C. Dedicated worker with a one-shot startup message (chosen)

The worker opens the OPFS database, seeds it if empty, reads the catalog, and
posts it back as a single string. The UI thread starts in a loading state and
moves to ready or failed when the message arrives.

- *Pro:* keeps SQLite where OPFS allows it, keeps the guarantees, keeps all
  application code in Rust, and gives the UI an honest loading/failure story.
- *Con:* an extra crate and build artifact; the catalog crosses a message
  boundary; the worker's script URL must be stable.

## Decision

1. **Startup sequence.** Both shells call
   `openrs_storage::load_or_seed_catalog(&mut repository, &Catalog::sample())`:
   seed only when the products table is empty, then load. The seed call is
   all-or-nothing — a variant whose product is missing leaves the database
   unchanged — and the second call returns `SeedOutcome::AlreadyPopulated`.
   After the first run the database is the only source of truth.
2. **Storage contract.** `Repository` gained `load_catalog` and
   `seed_catalog_if_empty`; both backends implement them, so the shared guarantee
   suite covers them natively and in Chromium. Only active products and their
   variants are loaded.
3. **Worker.** A new crate, `crates/openrs-web-worker`, owns the database. It is
   built by Trunk as a second wasm binary with `data-type="worker"` and a
   generated loader shim (`data-loader-shim="true"`), so no JavaScript is
   written by hand; the UI starts it with `web_sys::Worker::new`.
4. **Message format.** One string, `catalog\n<body>` or `error\n<reason>`, where
   the body is the line-based encoding in `openrs_core::wire` (header
   `openrs-catalog 1`, tab-separated fields, escaped). `serde`/`serde_json` were
   rejected to keep the dependency list at zero additions and because the codec
   is small enough to test exhaustively; reflecting over `js_sys` objects was
   rejected because it moves untyped data into the domain.
5. **Stable worker URL.** `Trunk.toml` sets `filehash = false`, so the shim is
   always `./openrs-web-worker_loader.js`. Trunk hashes file names by default,
   which would make the URL unknowable from compiled Rust.
6. **Cross-origin isolation.** `Trunk.toml` and the smoke test's static server
   both send `Cross-Origin-Opener-Policy: same-origin` and
   `Cross-Origin-Embedder-Policy: require-corp`, which `SharedArrayBuffer` needs.
   A static host must send the same pair.
7. **Desktop location.** `OPENRS_DATABASE` overrides the path; otherwise the file
   is `openrs.sqlite` in the platform data directory
   (`$XDG_DATA_HOME/openrs`, `~/Library/Application Support/OpenRS`,
   `%APPDATA%\OpenRS`).
8. **UI states.** `OpenRsApp` starts in `Loading`, and `set_catalog` /
   `set_error` move it to `Ready` / `Failed`. The failed state shows the reason,
   because an unreadable database must not resemble a store with no products.

## Consequences

- Desktop and web now load the same catalog from SQLite; the first run writes the
  development sample, later runs read what was stored.
- The web build has one more artifact and a worker round-trip at startup; the
  screen shows "Loading catalog…" while it happens.
- `filehash = false` is a deliberate tradeoff: stable names for the worker URL,
  at the cost of cache-busting. Revisit when the worker URL is resolved some
  other way (for example by reading the injected asset URL from the DOM).
- The protocol currently carries one message. The checkout workflow will need
  request/response messages in the worker; that extension will reuse this
  framing and needs its own ADR if it grows beyond a small request set.
- `web_sys::Worker` has no timeout in this implementation: a worker that never
  answers leaves the loading state on screen. The smoke test fails on that, so CI
  would catch a regression, but an in-app watchdog is a possible follow-up.
