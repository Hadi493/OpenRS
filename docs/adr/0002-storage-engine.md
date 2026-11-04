# ADR 0002: Storage engine and transaction model

- **Status:** **Accepted** by the project owner on 2026-11-02
- **Date:** 2026-11-02
- **Evidence:** `docs/phase-0.2-storage-spike-report.md`
- **Implementation:** `crates/openrs-storage`; results in
  `docs/phase-0.3-storage-layer-report.md`

## Context

`SPEC.md` requires atomic checkout, idempotent retry, and immutable sale
snapshots. `ROADMAP.md` left the engine open. Those requirements can only be
guaranteed by the storage layer, so the choice blocks the product catalog phase.

The repository must keep identical business outcomes on native desktop and web.
The project rules forbid webviews, Tauri/Electron, and JavaScript application
code, and `AGENTS.md` forbids `unsafe` by default.

A timeboxed spike compared an in-memory reference, native SQLite, browser SQLite,
and browser SQLite on OPFS. Results are in the report linked above.

## Options considered

### A. SQLite on both targets (recommended)

`rusqlite` natively; `sqlite-wasm-rs` + `sqlite-wasm-vfs` (`sahpool`/OPFS) on
web. One schema, one transaction model, one repository implementation per
platform adapter.

- Proven in the spike: the same DDL, `BEGIN IMMEDIATE`, `UNIQUE` idempotency key,
  and guarded stock decrement behave identically natively and in Chromium.
- Cost: OPFS needs a **dedicated worker** and **COOP/COEP headers**; the WASM
  bundle grows by ~1.15 MiB raw / ~474 KiB gzipped; WAL is unavailable on OPFS;
  the wrapper crate needs an explicit `unsafe` exception.

### B. SQLite native + IndexedDB web behind one trait

- No worker, no COOP/COEP, smaller bundle.
- Two storage implementations with different transaction semantics; the web side
  has no SQL transaction spanning the sale and the stock update. Idempotency and
  atomicity have to be re-established on top of IndexedDB, and the spike did not
  test this path.

### C. JSON files native + localStorage web

Rejected: weak atomicity and idempotency, unsuitable for sales and inventory.

### D. Remote server API

Rejected: contradicts the local-first initial scope; offline/sync is explicitly
later.

## Decision

Adopt **option A** with these conditions:

1. **One schema, shared.** `crates/openrs-storage` owns the migrations and the
   `Repository` implementation; neither shell sees SQL.
2. **Platform-split pragmas.** WAL + `synchronous = FULL` natively; rollback
   journal on OPFS (WAL is unsupported there). This lives behind a small
   platform configuration, not `cfg` branches inside each query.
3. **Dedicated web worker.** The browser database is owned by a worker (OPFS
   sync access handles do not exist on the main thread); the UI talks to it
   through a message-passing adapter with the same repository contract.
4. **One audited `unsafe` exception.** A single wrapper crate is the only place
   `unsafe` is allowed, and it documents why. `openrs-core` and both shells keep
   `unsafe_code = "forbid"`.
5. **UUIDv7 identifiers** for sales and checkout attempts. The spike showed that
   per-process sequences collide across connections.
6. **Money stays `i64` minor units + currency code**, no floating point.
7. **Idempotency is enforced in SQL** by `sales.checkout_id UNIQUE`; a retry
   returns the recorded sale and never deducts stock again.
8. **Snapshots are columns**, not joins: `sale_items` copies SKU, product name,
   unit price, and currency.

## Consequences

- Business behavior and validation live in one place; parity is testable with
  one shared suite on both targets.
- The web deployment must serve `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`, and host assets with compatible
  CORP/CORS headers. This affects the Trunk dev server and production hosting.
- The web app gains a worker and message-passing layer; the UI must handle
  asynchronous checkout results rather than blocking calls.
- Bundle size grows by roughly 0.5 MB gzipped. Acceptable against the existing
  6.2 MB wgpu bundle, but load time must be measured.
- Native SQLite (3.54.1) and WASM SQLite (3.53.4) versions differ; both targets
  must run the guarantee suite.
- `wasm-opt` becomes part of the release build once enabled.

## Decision record

Accepted in full: option A, the worker plus COOP/COEP deployment, the single
`unsafe` exception in `crates/openrs-storage/src/sqlite/wasm.rs`, and storage
checks in CI. Remaining open items are implementation questions, not engine
questions:

- The worker message protocol (request/response shape, cancellation, error
  mapping) is not designed yet; the storage layer is synchronous and is meant to
  be hosted inside the worker.
- `wasm-opt` and bundle-size budgets are still to be wired into the release
  build.
- Migrations beyond version 1 are not implemented; the version check exists so
  they can be added without changing the contract.
