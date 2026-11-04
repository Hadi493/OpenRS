# Phase 0.2 — Storage spike report

**Date:** 2026-11-02
**Status:** Spike complete. Engine decision **proposed**, not accepted — see
`docs/adr/0002-storage-engine.md`. No product code depends on this yet.
**Scope:** Throwaway spikes under `spikes/`. Nothing in `crates/` changed.

---

## 1. The question

`SPEC.md` makes three promises that are storage promises, not UI promises:

1. A failed checkout leaves no partial sale and no wrong stock count →
   **atomicity**.
2. Retrying the same checkout records and deducts once → **durable idempotency**.
3. A sale keeps the prices used at checkout after catalog edits →
   **immutable snapshots**.

`ROADMAP.md` left the engine open. The inspection report recommended spiking
**SQLite everywhere** (`rusqlite` natively, `sqlite-wasm-rs` + OPFS on web)
before committing, against **SQLite native + IndexedDB web** behind one
repository trait.

The spike asked: can one schema and one transaction model satisfy all three
guarantees on both targets, and what does it cost?

## 2. What was built

Three self-contained Cargo workspaces, deliberately detached from the product
workspace (each has an empty `[workspace]` table) so no spike dependency can
leak into `openrs-core` or the shells.

| Crate | Purpose |
|---|---|
| `spikes/storage` | `Repository` trait, `MemoryRepo` reference implementation, `SqliteRepo` on `rusqlite` (bundled SQLite), shared guarantee suite, file-backed concurrency tests, throughput measurement |
| `spikes/storage-wasm` | The same `migrations/0001_init.sql` executed in headless Chromium through `sqlite-wasm-rs` FFI; atomicity, rollback, `CHECK`, and `UNIQUE` idempotency asserted in-browser |
| `spikes/storage-wasm-opfs` | Installs the OPFS `sahpool` VFS in a dedicated worker; proves the database is written to OPFS, survives close/reopen, and records the journal mode available there |

The schema is one file shared by all three:

- `variants.stock CHECK (stock >= 0)` — stock cannot persist negative even if
  application logic is wrong.
- `sales.checkout_id UNIQUE` — the durable idempotency key.
- `sale_items` copies `sku`, `product_name`, `unit_price_minor`, `currency` — a
  sale never joins back to `variants`, so history is immutable.

Money is `i64` minor units plus a currency code. No floating point anywhere.

## 3. Environment

| Component | Version |
|---|---|
| Rust | 1.95.0 |
| Native SQLite | 3.54.1 (`rusqlite` 0.41.1 / `libsqlite3-sys` 0.39.1, bundled) |
| WASM SQLite | 3.53.4 (`sqlite-wasm-rs` 0.6.2, `sqlite-wasm-vfs` 0.3.0 with `sahpool`) |
| Browser | Chromium 156.0.8078.73, headless |
| wasm-bindgen CLI | 0.2.130 |
| wasm-pack | 0.15.1 |
| `wasm-opt` | **not installed** — size numbers below are pre-`wasm-opt` |

## 4. Results

### 4.1 Native (`spikes/storage`)

```bash
cd spikes/storage
cargo test --jobs 2
cargo clippy --all-targets --jobs 2 -- -D warnings
```

| Check | Result |
|---|---|
| 8 guarantee tests, each run against **both** backends | pass (16 backend executions) |
| 2 file-backed concurrency tests | pass |
| 1 throughput measurement | ignored by default; run explicitly |

The guarantee suite covers: sale recorded + stock decremented; insufficient
stock leaves nothing behind; a multi-line checkout fails atomically when the
second line is short; same key replays instead of duplicating; recorded sale is
immune to a later price edit; zero quantity rejected; unknown variant rejected;
totals exact in integer minor units.

Concurrency, with two independent connections on one database file:

- Two threads racing on the **same** checkout key → exactly one
  `Completed`, one `Replayed`, stock deducted once, one sale row.
- Two different checkouts each wanting 4 of 5 units → exactly one succeeds,
  stock ends at 1, never negative.

Throughput, file-backed with `journal_mode = WAL` and `synchronous = FULL`:

```
1000 durable checkouts in 148.729107ms (0.15 ms each)
```

That is roughly 6,700 checkouts/second on this machine, far above retail needs,
so `FULL` durability is affordable.

### 4.2 Browser, in memory (`spikes/storage-wasm`)

```bash
wasm-pack test --headless --chrome spikes/storage-wasm
```

5 tests, all pass in headless Chromium:

- SQLite compiles to `wasm32-unknown-unknown` and runs.
- The real `0001_init.sql` is accepted; all four tables exist.
- `BEGIN IMMEDIATE` + inserts + guarded `UPDATE` commits atomically; `ROLLBACK`
  restores stock; `CHECK (stock >= 0)` rejects negative stock.
- A duplicate `checkout_id` violates `UNIQUE` and no second row is inserted.
- Environment probe printed `crossOriginIsolated=true navigator_storage=true`
  under the `wasm-pack` test server.

### 4.3 Browser, persistent (`spikes/storage-wasm-opfs`)

```bash
wasm-pack test --headless --chrome spikes/storage-wasm-opfs
```

4 tests in a dedicated worker, all pass:

- The OPFS `sahpool` VFS installs and is registered with SQLite
  (`sqlite3_vfs_find("opfs-sahpool")` is non-null).
- A row written to an OPFS-backed database is still readable after closing and
  reopening the connection — the memory VFS could not do that.
- The real product schema applies to an OPFS database.
- **WAL is not available on OPFS:** `PRAGMA journal_mode = WAL` leaves the mode
  at `delete`. The native repository's pragmas cannot be reused verbatim.

### 4.4 Bundle-size cost

Same crate built with and without the `storage` feature, release profile,
`wasm-pack build`, no `wasm-opt`:

| Build | `.wasm` raw | `.wasm` gzip |
|---|---:|---:|
| `--no-default-features` (wasm-bindgen only) | 11,718 B | 5,365 B |
| with SQLite + OPFS VFS | 1,222,622 B | 490,482 B |
| **delta** | **+1,210,904 B (1.15 MiB)** | **+485,117 B (474 KiB)** |

Generated JS glue for the storage build is 18,307 B. The delta includes
`sqlite-wasm-rs`, `sqlite-wasm-vfs`, `wasm-bindgen-futures`, and their bindings,
so it is an upper bound for the storage layer. For context, the Phase 0.1 web
bundle was already 6.2 MB before any product code; storage adds roughly 20 %.

## 5. Findings

1. **One schema and one transaction model do satisfy all three guarantees on
   both targets.** The same SQL DDL, the same `BEGIN IMMEDIATE`, the same
   `UNIQUE` key, and the same guarded decrement behave identically natively and
   in Chromium.
2. **The SQLite versions differ.** Native bundles 3.54.1; the WASM build is
   3.53.4. Behavior used here is stable across both, but parity tests must run
   on both targets because the versions are not identical.
3. **OPFS needs a dedicated worker.** Sync access handles do not exist on the
   main thread. `eframe`'s web shell runs on the main thread, so adopting SQLite
   on web requires a worker-hosted database plus a message-passing adapter — an
   architectural cost, not a build detail.
4. **OPFS does not support WAL.** The shared repository layer must take its
   pragmas per platform: WAL + `synchronous = FULL` natively, rollback journal on
   OPFS. Without this the web path would silently fail or fall back.
5. **OPFS requires cross-origin isolation.** The `sahpool` VFS uses
   `SharedArrayBuffer`, so the web deployment must send
   `Cross-Origin-Opener-Policy: same-origin` and
   `Cross-Origin-Embedder-Policy: require-corp`. The tests only ever ran under
   the isolation-enabled `wasm-pack` server, so a plain static host is unproven.
6. **Sale IDs must be globally unique, not per-process sequences.** The native
   concurrency test had to share one id source; two independent per-process
   counters both produced `sale-1` and collided on the primary key. This
   confirms the inspection report's recommendation of UUIDv7-style ids.
7. **Unsafe FFI is unavoidable on web.** `sqlite-wasm-rs` is raw C bindings.
   The product's `unsafe_code = "forbid"` rule cannot hold for the one crate that
   wraps SQLite; that needs an explicit, documented exception.
8. **A safe async wrapper is real work.** The spike called the C API directly
   from tests. A product implementation needs a safe Rust wrapper, prepared
   statements instead of string-built SQL, and error mapping.

## 6. Known limitations

1. **No cross-reload test.** The OPFS tests prove persistence across connection
   close/reopen inside one worker. They do not reload the page or restart the
   worker; that needs a `wasm-bindgen-test`-independent harness.
2. **Chromium only.** Firefox and Safari OPFS support was not exercised.
3. **No multi-connection web test.** `sahpool` documents that concurrent
   connections and WAL are unsupported. Only one connection at a time was tested.
4. **Isolation was always on.** No test ran without COOP/COEP, so the header
   requirement is inferred from the implementation, not measured as a failure.
5. **Size numbers are pre-`wasm-opt`** and exclude gzip/brotli transfer tuning.
6. **The spikes are not in CI.** They are separate workspaces with heavy
   native and browser toolchain needs; wiring them in is a decision for the
   storage phase.
7. **Throughput is single-machine** (`llvmpipe`-class hardware, local disk) and
   is indicative only.

## 7. Recommendation

Adopt **SQLite on both targets** (option A) and record it in ADR 0002, with
these conditions:

- A platform-split pragma set: WAL natively, rollback journal on OPFS.
- A dedicated web worker owns the database; the UI talks to it through a
  message-passing repository adapter.
- One audited wrapper crate is the only place `unsafe` is permitted.
- UUIDv7 identifiers for sales and checkouts.
- The web deployment serves COOP/COEP headers and the size budget absorbs
  roughly 0.5 MB gzipped.

If the worker-plus-headers cost is unacceptable for the first web release,
the fallback is option B (SQLite native + IndexedDB web behind the same
`Repository` trait), which keeps the platform boundary but which the spike did
**not** test.

## 8. Reproduce

```bash
# Native: guarantees, concurrency, clippy
cd spikes/storage && cargo test --jobs 2 && cargo clippy --all-targets --jobs 2 -- -D warnings

# Native throughput
cd spikes/storage && cargo test --test sqlite_concurrency measure_checkout_throughput -- --ignored --nocapture

# Browser: in-memory SQLite and schema
wasm-pack test --headless --chrome spikes/storage-wasm

# Browser: OPFS persistence and journal mode
wasm-pack test --headless --chrome spikes/storage-wasm-opfs

# Bundle cost (release, no wasm-opt)
cd spikes/storage-wasm-opfs
wasm-pack build --release --no-default-features --out-dir pkg-baseline
wasm-pack build --release --out-dir pkg-storage
```
