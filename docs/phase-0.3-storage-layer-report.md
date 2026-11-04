# Phase 0.3 — Storage layer implementation report

**Date:** 2026-11-02
**Status:** Complete for the storage layer. No UI or product workflow uses it yet.
**Scope:** New `crates/openrs-storage`; the existing shells, UI, and core are
unchanged. The guarantee suite now runs natively and in a browser from the
product workspace.

---

## 1. What was built

`crates/openrs-storage` is a workspace member with no dependency on `egui`,
`eframe`, the filesystem, or the shells.

```text
crates/openrs-storage/
├── Cargo.toml                  # platform-specific SQLite deps, lint exception
├── migrations/0001_init.sql    # the schema proven by the spike
├── src/
│   ├── lib.rs                  # Repository contract + crate documentation
│   ├── model.rs                # value types, RepoError, Clock, IdSource
│   ├── memory.rs               # MemoryRepository, the behavioral reference
│   └── sqlite/
│       ├── mod.rs              # SQL statements + checkout algorithm (once)
│       ├── driver.rs           # Param/Value/Row + SqlSession trait
│       ├── native.rs           # rusqlite driver (WAL, synchronous = FULL)
│       └── wasm.rs             # sqlite-wasm-rs FFI driver + OPFS install
└── tests/
    ├── guarantees.rs           # shared suite: native cargo test + browser
    ├── native_file.rs          # file persistence + concurrency (native only)
    └── opfs.rs                 # OPFS persistence + replay (wasm only)
```

### How the guarantees are enforced

| Guarantee | Mechanism |
|---|---|
| No partial sale, no wrong stock | One `BEGIN IMMEDIATE` transaction covers the sale row, the line rows, and every stock decrement. Every early return rolls back. |
| Retry must not double-record | `sales.checkout_id UNIQUE`; the replay read happens inside the same transaction. |
| Sale keeps its prices | `sale_items` copies SKU, product name, unit price, and currency; reading a sale never joins to `variants`. |
| Stock never negative | `CHECK (stock >= 0)` plus a guarded `UPDATE ... WHERE stock >= ?1`; `changed == 0` aborts the transaction. |
| Exact money | `i64` minor units, checked multiply/add, no floating point. |
| One currency per sale | A mixed-currency checkout is rejected before any write. |

The SQL and the checkout algorithm exist once, in `sqlite/mod.rs`, and run
through a small `SqlSession` surface. The platform files only execute
statements; they contain no business logic.

## 2. Platform behavior

| Concern | Native | Browser |
|---|---|---|
| Driver | `rusqlite` 0.41.1, bundled SQLite 3.54.1 | `sqlite-wasm-rs` 0.6.2, SQLite 3.53.4 |
| Journal | WAL | rollback journal (OPFS does not support WAL) |
| Durability | `synchronous = FULL`, `busy_timeout = 5000` | `busy_timeout = 5000` |
| File location | caller-supplied path | OPFS pool via `SqliteRepository::open_opfs` |
| Threading | one connection per repository; concurrency tested across connections | must live in a dedicated worker |
| `unsafe` | none | confined to `sqlite/wasm.rs` |

`SqliteRepository::open_opfs` installs the OPFS VFS and is `async` because the
install talks to the browser file system through promises. It is intended to be
called from the worker that owns the database.

### Lint exception

The workspace forbids `unsafe`. `openrs-storage` restates the workspace lints and
downgrades only `unsafe_code` to `deny`, so `src/sqlite/wasm.rs` can opt in with
a documented `#![allow(unsafe_code)]`. Every other module and crate still
forbids `unsafe`. Cargo does not allow inheriting the workspace lint table and
overriding one entry, which is why the lints are restated; the `Cargo.toml`
comment records this.

## 3. Verification

All commands were run in this repository on 2026-11-02.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --jobs 2 -- -D warnings` | pass |
| `cargo clippy -p openrs-storage --target wasm32-unknown-unknown --all-targets -- -D warnings` | pass |
| `cargo test --workspace --jobs 2` | pass — **9 guarantee tests × 2 backends**, 3 native file/concurrency tests, existing core and UI tests |
| `wasm-pack test --headless --chrome crates/openrs-storage` | pass — **9 guarantee tests in Chromium** + **2 OPFS tests in a dedicated worker** |
| `cargo check -p openrs-storage --target wasm32-unknown-unknown` | pass |

The browser run proves the same guarantees as native: atomic multi-line
failure, insolvent stock rejection, replay after retry, immutable price
snapshots, exact totals, zero-quantity and unknown-variant rejection, and
mixed-currency rejection. The OPFS run proves the sale and stock survive a
closed connection and that a retry after reopening replays instead of recording a
second sale.

CI now checks the crate for `wasm32`, lints it for `wasm32`, and runs the browser
suite (`.github/workflows/ci.yml`). As with the rest of the workflow, this has
not yet run on a GitHub runner.

## 4. Deviations from the spike

- `variants.sku` is `NOT NULL UNIQUE` instead of nullable, because the domain
  type uses `String`. Making SKU optional is a product decision and would change
  the model.
- Currency mixing has a dedicated `CurrencyMismatch` error instead of
  `Storage`, and amount overflow has `AmountOverflow`. Both are now covered by
  the shared suite.
- The spike's per-process sequential ids are only used by tests. The product
  still needs a UUIDv7 `IdSource` implementation (ADR 0002 condition 5); it is
  not added yet because no caller exists and it would add a dependency without a
  user.
- `sales_created_at` is indexed ahead of the Phase 4 history screen.

## 5. Known limitations and next steps

1. **No UI wiring.** Neither shell opens a database; `openrs-ui` still renders
   the Phase 0.1 screen.
2. **No worker adapter.** The browser contract is synchronous and must be hosted
   in a worker; the message protocol, error mapping, and cancellation semantics
   are not designed yet.
3. **No UUIDv7 id source.** Sale ids are injected, so this is a small addition,
   but it needs the `uuid` dependency decision and WASM randomness check.
4. **Migrations stop at version 1.** The `user_version` check exists so versions
   can be added without changing the contract.
5. **Bundle size is untuned.** `wasm-opt` is still not in the release build.
6. **Native locking is tested on one machine only**, with two connections; it
   has not been exercised under a slow or networked file system.
7. **OPFS tests reuse one database per worker run.** They do not simulate a page
   reload or a second tab; `sahpool` documents that concurrent connections and
   WAL are unsupported.
