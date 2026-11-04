# Phase 1 — Product catalog report

**Date:** 2026-11-02
**Status:** Complete for a read-only catalog. The catalog is sample data, not yet
loaded from or saved to storage.
**Scope:** `openrs-core` catalog domain, `openrs-storage` type re-export, the
shared catalog screen in `openrs-ui`, tests, and docs.

---

## 1. What was built

### Domain (`crates/openrs-core/src/catalog.rs`)

| Item | Behavior |
|---|---|
| `Product` | id, name, flat category |
| `Variant` | id, product id, SKU, unit price in minor units, currency, stock |
| `StockStatus` | `Available` / `OutOfStock`; `is_out_of_stock` drives disabled rows |
| `Catalog` | immutable snapshot with deterministic order |
| `Catalog::find(query, category)` | trimmed, case-insensitive search over name, category, and SKU, combined with an optional category filter |
| `Catalog::sample()` | 4 products, 7 variants, 4 categories, 2 out-of-stock variants |
| `format_price` | exact minor-unit formatting (`1499 → "14.99 USD"`) |

Search semantics:

- Empty query, no category → every product with all its variants.
- Product name or category match → that product with **all** its variants, so
  searching "apparel" still shows every size.
- SKU match → only the matching variants, so searching "TEE-M" narrows the row.
- Unknown query → nothing; the screen shows an empty state, not an error.

### Type ownership (ADR 0001 amendment)

`Product`, `Variant`, and `MinorUnits` moved to `openrs-core` and are re-exported
by `openrs_storage::model`. The UI and storage now share one definition.
`openrs-storage` gained a path dependency on `openrs-core`; no new external
crates were added. Sale types stay in `openrs-storage` for now.

### Screen (`crates/openrs-ui/src/lib.rs`)

- Header, an on-screen note that the catalog is development sample data, a search
  box with a clear action, and category chips (`All` plus one per category).
- A result summary ("Showing N of M products"), then the scrollable list:
  product name, category, and per-variant SKU, price, and availability.
- Out-of-stock variants are rendered in a disabled scope **and** labelled
  "Out of stock", so availability is never color-only (`UI-UX.md`).
- Empty state: "No products match this search." plus a next-step hint.
- `with_catalog`, `set_query`, and `set_category` let shells and tests supply a
  snapshot or restore state without the UI knowing where it came from.

Both shells call `OpenRsApp::new()`, so desktop and web show identical rows from
identical Rust code. No shell or UI code contains SQL, and storage is not linked
into the UI.

## 2. Verification

All commands were run in this repository on 2026-11-02.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --jobs 2 -- -D warnings` | pass |
| `cargo check -p openrs-web --target wasm32-unknown-unknown` | pass |
| `cargo clippy -p openrs-storage --target wasm32-unknown-unknown --all-targets -- -D warnings` | pass |
| `cargo test --workspace --jobs 2` | pass — 12 core unit tests, catalog contract, 3 UI unit tests, 4 catalog screen tests, the existing 9 storage guarantee tests × 2 backends, 3 native file tests |
| `wasm-pack test --headless --chrome crates/openrs-core` | pass — catalog contract in Chromium |
| `scripts/smoke-web.sh` | pass — WASM bundle built, served, and `WebRunner` started in headless Chromium |
| `scripts/smoke-desktop.sh` | pass — native window rendered under Xvfb through `wgpu` |

### What the tests cover

- **Unit (core):** sample ordering and completeness, category list, empty query,
  trimming and case-insensitivity, SKU narrowing, exact category filtering,
  filter + query AND-combination, unknown query, out-of-stock and negative stock,
  product without variants, price formatting including `i64::MIN`.
- **Contract (native + Chromium):** the same assertion body runs as a plain test
  and as a `wasm_bindgen_test`. It pins product order, search results, category
  filtering, out-of-stock list, and price text, so the two targets cannot diverge
  silently.
- **Screen (`egui_kittest`):** renders products, SKUs, prices and stock text;
  typing in the search box filters the list; a category selection hides other
  categories; the empty state appears for an unmatched search; "Clear search"
  restores every product.

### Not covered

The screen tests run natively only. `egui_kittest` drives the shared widgets, and
the browser build is checked by `cargo check` plus the web smoke test, which
proves startup, not catalog contents. Reading the catalog text out of a WebGL
canvas would need a screenshot comparison, which is not worth the maintenance
cost yet.

## 3. Decisions made, and why

1. **Sample catalog lives in `openrs-core`, not storage.** Both targets must show
   the same catalog today, and loading a stored catalog in the browser needs the
   worker protocol that does not exist yet. A shared in-memory snapshot keeps
   parity honest instead of shipping a catalog on desktop and an empty screen on
   web. The screen says so.
2. **Catalog is read-only.** Creating and editing products is an open decision in
   `README.md` (read-only/imported catalog vs. full CRUD). Nothing here presumes
   an answer.
3. **Two decimal minor units with an assumption.** `MINOR_UNITS_PER_UNIT = 100`
   is documented, and `format_price` is the only place that assumption lives.
   A per-currency table needs a project-owner decision.
4. **Out-of-stock rows are disabled, not hidden.** `SPEC.md` wants unavailable
   items visible but not selectable; Phase 2 adds the selection.
5. **No `Product.active` or variant attributes yet.** Both need a schema column
   and a migration. Phase 1 only needs availability, which stock already
   provides.

## 4. Known limitations and next steps

1. **No persistence wiring.** The sample catalog is rebuilt on every start.
   Loading it from `openrs-storage` requires a synchronous read on desktop and
   the worker adapter on web, so both targets can adopt it at the same time.
2. **No catalog management.** No add, edit, or deactivate flows exist.
3. **No low-stock state.** Phase 5 owns thresholds; only available/out-of-stock
   exist now.
4. **Search is a linear scan.** Fine for a single store's catalog; revisit with a
   measured workload if catalogs reach tens of thousands of variants.
5. **Variant attributes are only in the SKU.** Size and color are not stored or
   displayed separately.
6. **The web catalog is not keyboard-navigated yet in a browser test.** The
   controls are standard egui widgets with labels, but no browser-level keyboard
   test exists.
