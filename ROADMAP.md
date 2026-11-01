# ROADMAP.md: OpenRS Roadmap

This roadmap describes the intended order of development. It is a working plan,
not a commitment to fixed dates. Each phase should be reviewed before the next
begins.

## Phase 0: Project foundations

- [ ] Create the Rust workspace and agree on the initial app structure.
- [ ] Confirm the required targets: desktop, web, or both for the first release.
- [ ] Add formatting, linting, and automated test checks.
- [ ] Write down project decisions that affect architecture.
- [ ] Decide what database to use for the first version.

**Done when:** The project builds, tests run, and the development setup is
documented.

## Phase 1: Product catalog

- [ ] Define product and product-variant types.
- [ ] Add sample products for development.
- [ ] Display products and variants in the app.
- [ ] Show stock and unavailable products.
- [ ] Add tests for product data and listing behavior.

**Done when:** A user can view a small catalog and identify which variants are
available.

## Phase 2: Bill creation and calculation

- [ ] Add items to a bill.
- [ ] Change quantities and remove items.
- [ ] Validate quantities and stock availability.
- [ ] Calculate line totals and subtotal.
- [ ] Show a bill preview.
- [ ] Add tests for normal, invalid, and boundary cases.

**Done when:** The bill preview shows correct totals and rejects invalid items
or quantities. Tax and discounts remain out of scope until their rules are
decided.

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
