//! Checkout: turning a confirmed bill into one recorded sale.
//!
//! Phase 3 of `ROADMAP.md` records a sale exactly once. The storage layer
//! (`openrs-storage::Repository::checkout`) owns the transaction, the stock
//! deduction, and the price snapshot; this module owns the two values both
//! shells and the storage layer must agree on:
//!
//! | Type | Purpose |
//! |---|---|
//! | [`CheckoutLine`] | one variant and quantity leaving the bill |
//! | [`CheckoutRequest`] | those lines plus the idempotency key |
//! | [`CheckoutReceipt`] | the recorded sale shown back to the user |
//!
//! # The idempotency key
//!
//! [`CheckoutRequest::checkout_id`] is generated **once per checkout attempt**
//! and reused when that attempt is retried. The storage layer makes it unique
//! in the database, so a retry after a lost reply, a double click, or a stalled
//! worker returns the sale that was already recorded instead of writing a
//! second one. Generating the key before the first write is what makes the
//! retry safe; regenerating it on retry would defeat the guarantee.
//!
//! # The receipt
//!
//! The UI does not need a full [`crate::Bill`] back after checkout: it clears
//! the bill and shows what was recorded. [`CheckoutReceipt`] is therefore the
//! smallest shared shape that both shells can produce - the desktop from
//! `openrs_storage::Sale`, the web shell from the storage worker message in
//! [`crate::wire`].
//!
//! # Platform parity
//!
//! This module is pure Rust. The only platform-specific part is the entropy
//! behind [`new_checkout_id`], which `uuid` supplies through the operating
//! system on desktop and WebCrypto in the browser.

use crate::bill::Bill;

use crate::catalog::{MinorUnits};
use crate::wire::VariantId;
use crate::wire::{PriceMinor, TotalMinor, Currency, ReceiptLine};

/// One line of a checkout: the variant and how many units leave stock.
///
/// A checkout line deliberately carries no price. The storage layer reads the
/// price, SKU, and product name at checkout time and copies them into the sale,
/// so a client cannot propose its own price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutLine {
    /// Variant being sold.
    pub variant_id: String,
    /// Quantity to sell. Must be greater than zero; storage rejects zero.
    pub quantity: u32,
}

/// A checkout attempt, identified by its idempotency key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutRequest {
    /// Idempotency key. See the module documentation.
    pub checkout_id: String,
    /// Lines to sell, in bill order.
    pub lines: Vec<CheckoutLine>,
}

impl CheckoutRequest {
    /// Builds a request from a confirmed [`Bill`].
    ///
    /// The lines are copied in the order the bill shows them. An empty bill
    /// produces an empty request; storage rejects it with
    /// `RepoError::EmptyCheckout`, so a caller that forgets to check cannot
    /// record a zero-line sale.
    ///
    /// Prices are **not** copied: storage prices every line from the variant
    /// rows, which is what keeps the recorded sale consistent with the stock
    /// deduction.
    #[must_use]
    pub fn for_bill(checkout_id: impl Into<String>, bill: &Bill) -> Self {
        Self {
            checkout_id: checkout_id.into(),
            lines: bill
                .lines()
                .iter()
                .map(|line| CheckoutLine {
                    variant_id: line.variant_id.clone(),
                    quantity: line.quantity,
                })
                .collect(),
        }
    }

    /// Returns whether the request would sell nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// What the user is told after a sale was recorded.
///
/// A replayed attempt produces the same receipt values as the original sale,
/// because both come from the stored sale.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckoutReceipt {
    /// Sale identifier, stable for the life of the sale.
    pub sale_id: String,
    /// Idempotency key that produced the sale.
    pub checkout_id: String,
    /// Final total, in minor units.
    pub total_minor: MinorUnits,
    /// Currency of the total.
    pub currency: String,
    /// Lines copied from the catalog at checkout time, so a receipt never
    /// changes when the catalog does.
    pub sale_items: Vec<ReceiptLine>,
}

/// Lines rendered on a receipt; they describe the sold variants and can
/// never be rebuilt from the currency or the checkout key alone.


#[derive(Debug, Clone, PartialEq)]
pub struct ReceiptLine {
    pub variant_id: VariantId,
    pub variant_name: String,
    pub quantity: MinorUnits,
    pub unit_price_minor: PriceMinor,
    pub total_minor: MinorUnits,
}

impl Eq for ReceiptLine {}

impl Ord for ReceiptLine {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.total_minor.cmp(&other.total_minor)
    }
}

impl PartialOrd for ReceiptLine {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Generates a new idempotency key for one checkout attempt.
///
/// The value is a UUIDv7: time-ordered, so keys sort by attempt time, and
/// random enough that two tabs, two processes, or a restart cannot collide.
/// Uniqueness matters more than readability here - a collision would make the
/// storage layer replay a different sale instead of recording a new one.
#[must_use]
impl ReceiptLine {
    pub fn new(variant_id: VariantId, variant_name: String, quantity: MinorUnits, unit_price_minor: PriceMinor, total_minor: MinorUnits) -> Self {
        Self {
            variant_id,
            variant_name,
            quantity,
            unit_price_minor,
            total_minor,
        }
    }
}

pub fn new_checkout_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::{CheckoutLine, CheckoutRequest, new_checkout_id};
    use crate::bill::Bill;
    use crate::catalog::Catalog;

    #[test]
    fn a_bill_becomes_one_line_per_bill_line() {
        let catalog = Catalog::sample();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-tee-s", 2).expect("add t-shirt");
        bill.add(&catalog, "v-beans", 1).expect("add beans");

        let request = CheckoutRequest::for_bill("checkout-1", &bill);

        assert_eq!(request.checkout_id, "checkout-1");
        assert_eq!(
            request.lines,
            vec![
                CheckoutLine {
                    variant_id: "v-tee-s".to_owned(),
                    quantity: 2,
                },
                CheckoutLine {
                    variant_id: "v-beans".to_owned(),
                    quantity: 1,
                },
            ]
        );
    }

    #[test]
    fn an_empty_bill_produces_an_empty_request() {
        let request = CheckoutRequest::for_bill("checkout-1", &Bill::new());

        assert!(request.is_empty());
        assert_eq!(request.lines.len(), 0);
    }

    #[test]
    fn a_quantity_change_is_reflected_in_the_request() {
        let catalog = Catalog::sample();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-mug-grey", 1).expect("add");
        bill.set_quantity(&catalog, "v-mug-grey", 3).expect("set");

        let request = CheckoutRequest::for_bill("checkout-1", &bill);

        assert_eq!(request.lines[0].quantity, 3);
    }

    #[test]
    fn checkout_keys_are_unique() {
        let keys: std::collections::BTreeSet<String> =
            (0..1_000).map(|_| new_checkout_id()).collect();

        assert_eq!(keys.len(), 1_000, "every attempt needs its own key");
    }

    #[test]
    fn checkout_keys_are_uuid_v7() {
        let key = new_checkout_id();
        let parsed = uuid::Uuid::parse_str(&key).expect("a checkout key must be a UUID");

        assert_eq!(parsed.get_version_num(), 7, "UUIDv7 is time-ordered");
        assert_eq!(key.len(), 36);
    }
}
