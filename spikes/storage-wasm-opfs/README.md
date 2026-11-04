# Spike: SQLite persistence in OPFS

**Status:** investigation only — nothing here is product code.
**Question:** can the web build keep its sales database across sessions, or is
browser SQLite memory-only?

The crate installs the OPFS `sahpool` VFS from `sqlite-wasm-vfs` and asserts, in
a dedicated worker in headless Chromium:

- the VFS installs and is registered with SQLite;
- a row written to an OPFS database survives closing and reopening the
  connection (the memory VFS could not do that);
- the real product schema applies to an OPFS database;
- `PRAGMA journal_mode = WAL` leaves the mode at `delete` — **WAL is not
  available on OPFS**, so the native pragmas cannot be reused verbatim.

It also builds with and without the `storage` feature so the WebAssembly size
cost of the storage stack can be measured rather than estimated. Measured on
2026-11-02: +1.15 MiB raw / +474 KiB gzipped (no `wasm-opt`).

OPFS sync access handles only exist in a dedicated worker, so the tests use
`run_in_dedicated_worker`, and the `sahpool` VFS needs cross-origin isolation
(`COOP`/`COEP`) because it uses `SharedArrayBuffer`.

## How to run

```bash
# Tests
wasm-pack test --headless --chrome spikes/storage-wasm-opfs

# Size measurement
cd spikes/storage-wasm-opfs
wasm-pack build --release --no-default-features --out-dir pkg-baseline
wasm-pack build --release --out-dir pkg-storage
```

Full results and the proposed engine decision:
`docs/phase-0.2-storage-spike-report.md` and `docs/adr/0002-storage-engine.md`.
