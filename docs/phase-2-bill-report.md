# Phase 2: Bill creation and calculation

## Overview

Phase 2 adds the pending bill (basket) that a cashier builds before checkout.
A bill is an in-memory working set: the user adds catalog variants, adjusts
quantities, removes lines, and reviews the exact subtotal required by `BILL-001`.
Recording the sale remains Phase 3.

## What was done

- **`openrs_core::bill`** — a pure-Rust `Bill` type shared by both targets. It
  holds at most one line per variant, merges repeated adds, and exposes
  `add`, `set_quantity`, `remove`, `clear`, `lines`, `subtotal_minor`, and
  `currency`.
- **Copied line values** — `BillLine` copies the SKU, product name, unit price,
  and currency from the catalog when the line is added, so the preview does not
  change under the user when the catalog is re-read. Checkout still re-reads the
  price inside its transaction; that is the price a sale is recorded with.
- **Validation before mutation** — quantity must be positive, at most the
  available stock, and in the same currency as the rest of the bill. Line totals
  and the subtotal use checked `i64` arithmetic. A rejected call leaves the bill
  untouched.
- **`BillError`** — mirrors the storage rules (`UnknownVariant`,
  `NonPositiveQuantity`, `InsufficientStock`, `CurrencyMismatch`,
  `AmountOverflow`) so the UI can report a rejection before checkout is
  attempted.
- **Bill panel in `openrs-ui`** — a right-hand panel lists each line with its
  quantity, unit price, and line total, offers `Decrease`, `Increase`, and
  `Remove` per line, shows the subtotal, and offers `Clear bill`. An empty bill
  says so and shows `Subtotal: 0.00`.
- **Add actions** — every in-stock variant row has an `Add <SKU>` button.
  Out-of-stock rows keep it disabled, and availability remains written as text.
- **Error reporting** — a rejected change is shown in the bill panel next to the
  bill, without turning the whole screen into the catalog failure state.
- **Tests** — unit tests for the domain and `egui_kittest` screen/interaction
  tests, plus a cross-target `bill_contract` acceptance test that runs natively
  and in Chromium.

## Why the bill is not in the storage layer

A bill is not persisted; only a completed sale is. Putting `Bill` in
`openrs-core` keeps the quantity, stock, and currency rules in shared domain code
without adding storage methods or SQL that both backends would have to
implement. `checkout` already owns atomicity, idempotency, and price snapshots,
so Phase 3 only has to convert a confirmed bill into a `CheckoutRequest`.

The bill requires one currency because checkout rejects a mixed-currency sale.
Enforcing the same rule in the preview means the UI cannot show a bill that
checkout would refuse.

## Test results

- `cargo test --workspace` — 88 tests pass: 40 `openrs-core` unit tests (21 for
  the bill), 1 native bill contract, 1 catalog contract, 1 foundation contract,
  5 desktop startup, 13 storage guarantees, 4 native-file storage tests, 18 UI
  tests (11 bill-specific, 3 of them clicking the actual buttons), 4 catalog
  screen tests, and 1 storage doctest.
- `wasm-pack test --headless --chrome crates/openrs-core` — `bill_contract`,
  `catalog_contract`, and `foundation_contract` pass in Chromium.
- `cargo clippy --workspace --all-targets` — no warnings.
- `cargo fmt --all -- --check` — clean.

## Consequences

- Both shells show the same bill because the rules and arithmetic live in shared
  Rust; the platform shells still only call `OpenRsApp`.
- The bill survives a catalog refresh (`set_catalog`) because its lines carry
  the values they were added with. A line whose variant later disappears can
  still be removed, but changing its quantity reports `UnknownVariant`.
- The bill is lost when the app closes. That is expected until a draft-bill
  persistence decision is made.
- `Decrease` stops at one; removing the last unit uses the explicit `Remove`
  button. This keeps "empty the line" a deliberate action.

## Open questions / follow-ups

- Should a draft bill survive an application restart? This is a persistence and
  recovery decision for a later phase.
- When checkout lands, the confirmed bill needs an idempotency key per attempt,
  reused on retry (already required by `CheckoutRequest::checkout_id`).
- Line notes, manual price overrides, and discounts are not modelled; tax and
  discount rules remain unscheduled in `ROADMAP.md`.
