# SPEC.md: OpenRS Retail Software

## Purpose

OpenRS is retail software for listing products, creating bills, completing sales,
and tracking inventory.

This document defines the initial scope and behavior. Features marked “Later”
are not part of the first working release.

## Initial Scope

The first release should support a basic sale using locally stored product and
inventory data. It should calculate a bill, record a completed sale, update
inventory, and let the user review or export the receipt.

Payment processing, tax rules, and customer accounts require separate decisions
before implementation.

## Core Data

### Product

A product should have:

- Unique product ID
- Name
- Category
- One or more variants
- Optional image reference
- Active/inactive status

### Product variant

A variant represents a sellable option, such as a size or color. It should have:

- Unique variant ID
- Product ID
- SKU or barcode, if used
- Variant attributes, such as size
- Unit price
- Stock quantity

Prices must use an exact representation, such as integer minor units, together
with a currency code. Do not use floating-point values for money.

### Sale

A sale should record:

- Unique sale ID
- Date and time
- Purchased items and quantities
- Unit prices at the time of sale
- Subtotal, discounts, tax, and final total, when applicable
- Sale status

The sale must retain the prices used at checkout even if product prices change later.

## Customer and Staff Workflows

### Product listing

- Show active products and their available variants.
- Allow searching or filtering by name, category, SKU, or barcode as supported.
- Show when a variant is out of stock.
- Do not allow an inactive or unavailable item to be added to a sale.

### Bill creation

- Allow items to be added to and removed from a bill.
- Allow quantities to be changed.
- Validate that quantities are positive and do not exceed available stock.
- Show a bill preview before the sale is completed.

### Bill calculation

- Calculate each line total and the bill subtotal.
- Apply discounts and tax only according to rules approved for the project.
- Show the calculation clearly in the preview.
- Define and test rounding behavior before implementing tax or discounts.

### Sale completion and inventory update

- Completing a sale must record the sale and update inventory consistently.
- A failed checkout must not leave a partial sale or an incorrect stock count.
- Retrying the same checkout must not record the sale or deduct stock twice.
- Show a clear success or failure result to the user.

### Receipt

- Provide a printable preview.
- Allow export in an agreed format.
- Include the sale ID, date, items, quantities, prices, and totals.
- Do not expose internal database details in the receipt.

### History

- Allow staff to view and search past sales.
- A sale’s saved totals and item prices must not change when the product catalog changes.
- Return, refund, and cancellation behavior is not included until specified.

### BILL-001: Calculate bill subtotal

The system shall calculate the subtotal from the items and quantities in the bill.

Applies to: Desktop, Web
Verification: Shared Rust unit tests; desktop and WebAssembly acceptance tests.

## Later Features

- Low-stock notifications
- Out-of-stock workflows
- Returns and refunds
- Multiple locations
- Customer accounts
- Payment-provider integration
- Offline checkout and synchronization
- Automated tax rules

## Acceptance Tests

Before calling the initial release complete, test that:

- A valid sale calculates the expected totals.
- Invalid quantities and unavailable items are rejected.
- Completing a sale records it and updates stock correctly.
- A failed or repeated checkout does not corrupt inventory or create duplicates.
- A saved sale remains unchanged after its product’s price is edited.
- Receipt preview and export show the same sale details.
