# Spike: SQLite in the browser

**Status:** investigation only — nothing here is product code.
**Question:** does the SQLite C library compile and run under
`wasm32-unknown-unknown`, and do the `SPEC.md` schema guarantees hold in a real
browser?

The tests load the *same* `../storage/migrations/0001_init.sql` used by the
native spike and assert, in headless Chromium:

- SQLite runs at all;
- all four product tables are created;
- `BEGIN IMMEDIATE` + inserts + a guarded stock decrement commits atomically,
  `ROLLBACK` restores stock, and `CHECK (stock >= 0)` rejects negative stock;
- a duplicate `checkout_id` violates `UNIQUE` and inserts no second sale;
- the deployment environment (cross-origin isolation, OPFS availability) is
  reported to the console.

Everything here is direct `unsafe` FFI. That is a finding in itself: adopting
`sqlite-wasm-rs` in the product needs a safe wrapper crate and an explicit
exception to the project's `unsafe_code = "forbid"` rule.

## How to run

```bash
wasm-pack test --headless --chrome spikes/storage-wasm
```

The in-memory database says nothing about persistence. See
`spikes/storage-wasm-opfs` for the OPFS evidence and
`docs/phase-0.2-storage-spike-report.md` for the conclusion.
