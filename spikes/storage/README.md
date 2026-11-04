# Spike: storage engine

**Status:** investigation only — nothing here is product code.
**Question:** can one SQLite schema and one transaction model satisfy the
`SPEC.md` sale guarantees on both the native and the WebAssembly target?

## Why this exists

`SPEC.md` requires three properties that are storage properties, not UI
properties:

- *“A failed checkout must not leave a partial sale or an incorrect stock
  count.”* → atomicity.
- *“Retrying the same checkout must not record the sale or deduct stock twice.”*
  → a durable idempotency key with a uniqueness guarantee.
- *“A sale must retain the prices used at checkout even if product prices change
  later.”* → immutable snapshot columns on the sale line, not joins.

`ROADMAP.md` leaves the engine open. This spike exists to produce evidence for
that decision instead of guessing.

## Method

`spikes/storage` is a **separate Cargo workspace** (note the empty `[workspace]`
table in its `Cargo.toml`). It is deliberately not a member of the product
workspace, so adding `rusqlite` here cannot leak into `openrs-core` or either
shell.

The spike defines a `Repository` trait with two implementations:

- `memory::MemoryRepo` — the reference semantics, in plain Rust.
- `sqlite::SqliteRepo` — the candidate engine.

One shared test suite (`tests/guarantees.rs`) runs the same scenarios against
both. If the two implementations disagree, the trait is under-specified or SQLite
cannot express the guarantee.

## How to run

```bash
cd spikes/storage
cargo test --jobs 2
```

## Findings

Both implementations pass the same eight guarantee tests, and the file-backed
concurrency tests show that two connections racing on one checkout key record
exactly one sale, while two competing checkouts cannot oversell. A durable
checkout (WAL + `synchronous = FULL`) costs about 0.15 ms on this machine.

Full results, browser evidence, and the proposed engine decision:

- `docs/phase-0.2-storage-spike-report.md`
- `docs/adr/0002-storage-engine.md`
