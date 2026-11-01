# UI-UX.md: UI/UX of OpenRS

## Purpose

This document defines the user-interface and interaction principles for OpenRS.
It is a working guide for design and implementation, not a fixed visual theme.

OpenRS supports a native desktop app and a WebAssembly web app. Both should
provide the same agreed product workflows and business behavior.

## Design Principles

- **Make common work fast:** Product search, adding items, editing quantities,
  and reviewing a bill should require as few clear steps as practical.
- **Make important information obvious:** Prices, quantities, stock status, and
  bill totals must be easy to find.
- **Prevent costly mistakes:** Make the current action and its result clear,
  especially when completing, cancelling, or changing a sale.
- **Use consistent patterns:** Similar actions, controls, and messages should
  behave alike throughout the app.
- **Be accessible:** Support keyboard navigation, readable text, visible focus,
  and clear labels. Do not communicate status through color alone.
- **Be honest about system state:** Show when data is loading, saved, unavailable,
  or failed to save.

## Platform Parity

- Desktop and web builds must support the same approved workflows and business
  rules.
- Layout and input details may adapt to the platform, but the meaning and result
  of an action must remain the same.
- Prefer shared Rust application logic and shared validation rules.
- Document any approved platform-specific difference, why it exists, and whether
  it is temporary or permanent.
- A feature is not UI-complete until its relevant desktop and web behavior has
  been tested.

## Main Retail Workflow

The primary workflow is:

1. Find and select a product variant.
2. Add it to the current bill.
3. Review items, quantities, and prices.
4. Review the bill total and any applicable adjustments.
5. Confirm the sale.
6. Show a clear completion state and offer receipt actions.

The user must be able to tell whether a sale is still being edited, has completed,
or has failed. Do not make checkout appear successful until the sale has actually
been recorded.

## Product Listing

- Show product names, relevant variant details, prices, and stock status.
- Make search and category filtering easy to find.
- Clearly distinguish available, low-stock, and out-of-stock variants.
- Do not let unavailable products appear selectable without an explicit reason.
- Preserve useful search or filter state when returning from a product detail
  view, where practical.

## Bill and Checkout

- Keep the current bill visible while the user adds or edits items.
- Show each item’s description, quantity, unit price, and line total.
- Make quantity changes and item removal straightforward.
- Display subtotal, discounts, tax, and final total as separate values when
  those features are enabled.
- Ask for confirmation before an irreversible action or a destructive action
  that could lose the current bill.
- Disable or clearly block checkout while required validation or saving is
  incomplete.
- If an action fails, explain what happened and how the user can recover without
  implying that the sale completed.

## Feedback and UI States

Every screen or major component should account for applicable states:

- Initial or empty state
- Loading or processing state
- Success state
- Validation or input error
- Save or network failure
- Unavailable or out-of-stock state

Messages should say what happened in plain language and provide a useful next
action when possible. Avoid exposing internal errors, database details, or
technical traces in normal user-facing messages.

## Accessibility and Input

- Every interactive control must have a clear label and purpose.
- Keyboard users must be able to reach and operate important controls.
- Focus should be visible and should move in a predictable order.
- Text and controls must remain legible at supported window sizes and display
  scaling.
- Use text or icons alongside color to communicate status.
- Avoid interactions that require precise pointer movement when a simpler
  alternative is practical.
- Keep error messages near the relevant field or action.

## Visual Consistency

- Use shared styles for spacing, typography, controls, status indicators, and
  dialogs.
- Keep primary actions visually distinct from secondary actions.
- Use confirmation dialogs consistently and only when they prevent meaningful
  mistakes.
- Avoid adding decoration that competes with product, price, stock, or bill
  information.
- Record significant design-system changes so both platform implementations
  remain consistent.

## UI Testing

For each significant workflow, verify:

- The workflow can be completed on desktop and web.
- Keyboard navigation reaches and operates the relevant controls.
- Empty, loading, success, and failure states behave clearly.
- Invalid input is identified and does not silently change the bill.
- Displaying the same sale on both platforms yields the same item details and
  totals.
- Approved platform differences are documented and tested.

## Open Design Decisions

Decide these before they become difficult to change:

- Whether the primary use is mouse, keyboard, touch, or a mix.
- The minimum supported window and screen sizes.
- Which receipt formats and print behaviors are required.
- Whether the app needs a dark theme.
- Which user roles and permissions are needed.
