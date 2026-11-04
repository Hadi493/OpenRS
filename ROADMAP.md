# ROADMAP.md: OpenRS Roadmap

This roadmap describes the intended order of development. It is a working plan,
not a commitment to fixed dates. Each phase should be reviewed before the next
begins.

## Phase 0: Project foundations

- [x] Create the Rust workspace and agree on the initial app structure.
- [x] Confirm the required targets: desktop, web, or both for the first release.
- [x] Add formatting, linting, and automated test checks.
- [x] Write down project decisions that affect architecture.
- [x] Decide what database to use for the first version. *(SQLite on both
  targets; ADR 0002 accepted, implemented in `crates/openrs-storage`.)*

**Done when:** The project builds, tests run, and the development setup is
documented.

## Phase 1: Product catalog

- [x] Define product and product-variant types. *(Shared in `openrs-core`;
  storage re-exports them.)*
- [x] Add sample products for development. *(`Catalog::sample`; not persisted
  yet — see the Phase 1 report.)*
- [x] Display products and variants in the app. *(Shared catalog screen in
  `openrs-ui`, rendered by both shells.)*
- [x] Show stock and unavailable products. *(Out-of-stock variants are listed as
  disabled rows with an explicit “Out of stock” label.)*
- [x] Add tests for product data and listing behavior. *(Unit tests, a
  native + Chromium contract test, and `egui_kittest` screen tests.)*

**Done when:** A user can view a small catalog and identify which variants are
available. *(Met with the shared sample catalog. Loading a stored catalog is
blocked on the web worker protocol and is the next catalog step.)*

## Phase 2: Bill creation and calculation

- [x] Add items to a bill.
- [x] Change quantities and remove items.
- [x] Validate quantities and stock availability.
- [x] Calculate line totals and subtotal.
- [x] Show a bill preview.
- [x] Add tests for normal, invalid, and boundary cases.

**Done when:** The bill preview shows correct totals and rejects invalid items
or quantities. Tax and discounts remain out of scope until their rules are
decided. *(Met by `openrs_core::Bill`, the bill panel in `openrs-ui`, and the
native + WASM `bill_contract` acceptance test. The bill is in-memory only;
recording the sale is Phase 3.)*

## Phase 3: Complete and record a sale

- [ ] Define the sale record and its statuses.
- [ ] Save a completed sale.
- [ ] Update inventory consistently with the sale.
- [ ] Handle errors without leaving a partial sale or incorrect stock.
- [ ] Prevent retries from creating duplicate sales or stock deductions.
- [ ] Add tests for successful, failed, and repeated checkout attempts.

**Done when:** A completed sale is recorded once, inventory is correct, and
failures can be retried safely.

## Phase 4: Receipts and sales history

- [ ] Create a printable receipt preview.
- [ ] Export a receipt in the chosen format.
- [ ] Show past sales.
- [ ] Confirm saved sale details do not change when product data changes.
- [ ] Add tests for receipt contents and history behavior.

**Done when:** Staff can review a past sale and produce a receipt with the
recorded sale details.

## Phase 5: Stock alerts

- [ ] Decide how low-stock thresholds are configured.
- [ ] Show low-stock and out-of-stock items.
- [ ] Add tests for threshold and restocking behavior.

**Done when:** Staff can identify products that need restocking.

## Phase 6: Broader platform and release readiness

- [ ] Confirm the shared Rust logic works on each supported target.
- [ ] Test the desktop and web interfaces on their intended platforms.
- [ ] Document installation, backup, and basic recovery procedures.
- [ ] Review error handling, accessibility, and performance.
- [ ] Fix release-blocking issues before calling the version ready.

**Done when:** The supported build targets pass their checks and a new user can
build and use the app by following the documentation.

## Not scheduled yet

These need more decisions before implementation:

- Tax rules and rounding
- Discounts
- Payment processing
- Returns and refunds
- Multiple store locations
- Offline checkout and synchronization
- Customer accounts
