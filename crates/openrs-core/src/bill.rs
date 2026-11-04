//! Pending bill (basket): the editable set of items before checkout.
//!
//! Phase 2 of `ROADMAP.md` is bill creation and calculation. A [`Bill`] is a
//! pre-checkout working set: the user adds variants, changes quantities, and
//! removes lines, and the UI previews the line totals and subtotal. Nothing
//! here is persisted; `openrs-storage::Repository::checkout` is what records a
//! sale atomically once the bill is confirmed (Phase 3).
//!
//! # What is copied and what is validated
//!
//! When a line is added, the SKU, product name, unit price, and currency are
//! copied from the catalog into the [`BillLine`]. The preview therefore shows
//! exactly what the user selected even if the catalog is re-read mid-session.
//! Stock and quantity rules are validated against the supplied [`Catalog`] at
//! the moment of each change, so an impossible bill cannot be built:
//!
//! | Rule | Where enforced |
//! |---|---|
//! | Quantity is greater than zero | `add` and `set_quantity` |
//! | Quantity never exceeds available stock | `add` and `set_quantity` |
//! | One currency per bill | `add` and `set_quantity` |
//! | Line total and subtotal stay exact | checked `i64` arithmetic |
//! | An out-of-stock variant cannot be added | `add` |
//!
//! # Why one currency per bill
//!
//! Checkout rejects a mixed-currency sale (`RepoError::CurrencyMismatch`).
//! Enforcing the same rule here means the preview can never show a bill that
//! checkout would refuse, which is the "prevent costly mistakes" principle in
//! `UI-UX.md`.
//!
//! # Platform parity
//!
//! Everything in this module is pure Rust with no platform calls, so the
//! desktop and web targets run the same validation and arithmetic. The unit
//! tests below run natively and, through the crate's WASM test target, in the
//! browser.

use std::fmt;

use crate::catalog::{Catalog, MINOR_UNITS_PER_UNIT, MinorUnits, Variant};

/// Why a bill change was rejected.
///
/// The variants mirror the storage-layer errors the same rules produce at
/// checkout, so the UI can report the same reasons before the sale is attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillError {
    /// The variant is not in the catalog.
    UnknownVariant(String),
    /// A quantity of zero was supplied where a positive quantity is required.
    NonPositiveQuantity {
        /// Offending variant.
        variant_id: String,
    },
    /// The requested quantity exceeds available stock.
    InsufficientStock {
        /// Offending variant.
        variant_id: String,
        /// Stock available at the time of the change.
        available: i64,
        /// Quantity the bill would hold.
        requested: u32,
    },
    /// The line would mix two currencies, which checkout does not support.
    CurrencyMismatch {
        /// Offending variant.
        variant_id: String,
        /// Currency already used by the bill.
        expected: String,
        /// Currency the offending variant uses.
        found: String,
    },
    /// A line total or the running subtotal overflowed `i64`.
    AmountOverflow {
        /// Offending variant.
        variant_id: String,
    },
}

impl fmt::Display for BillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownVariant(id) => write!(f, "unknown variant `{id}`"),
            Self::NonPositiveQuantity { variant_id } => {
                write!(f, "quantity for `{variant_id}` must be greater than zero")
            }
            Self::InsufficientStock {
                variant_id,
                available,
                requested,
            } => write!(
                f,
                "insufficient stock for `{variant_id}`: requested {requested}, available {available}"
            ),
            Self::CurrencyMismatch {
                variant_id,
                expected,
                found,
            } => write!(
                f,
                "variant `{variant_id}` uses currency `{found}` but the bill uses `{expected}`"
            ),
            Self::AmountOverflow { variant_id } => {
                write!(f, "amount overflow while pricing `{variant_id}`")
            }
        }
    }
}

impl std::error::Error for BillError {}

/// One line of a bill, with the values copied from the catalog when it was
/// added.
///
/// The copies are deliberate: a later catalog price edit does not silently
/// change the bill the user is reviewing. Checkout still re-reads the current
/// price inside its transaction, which is the price a sale is recorded with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillLine {
    /// Variant being billed.
    pub variant_id: String,
    /// SKU at the time the line was added.
    pub sku: String,
    /// Product name at the time the line was added.
    pub product_name: String,
    /// Unit price at the time the line was added, in minor units.
    pub unit_price_minor: MinorUnits,
    /// Currency of [`Self::unit_price_minor`].
    pub currency: String,
    /// Quantity on the bill. Always greater than zero.
    pub quantity: u32,
    /// `unit_price_minor * quantity`, kept exact and precomputed.
    pub line_total_minor: MinorUnits,
}

/// A pending bill: an ordered set of [`BillLine`]s and its exact subtotal.
///
/// A bill holds at most one line per variant. Adding a variant that is already
/// present merges the quantities, which is what "add another of the same item"
/// means to a cashier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bill {
    lines: Vec<BillLine>,
    subtotal_minor: MinorUnits,
}

impl Bill {
    /// Creates an empty bill.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the bill's lines in insertion order.
    #[must_use]
    pub fn lines(&self) -> &[BillLine] {
        &self.lines
    }

    /// Returns `true` when the bill has no lines.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Returns how many distinct variants the bill holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Returns the exact subtotal in minor units.
    ///
    /// This is the number `BILL-001` requires: the sum of the line totals. Tax
    /// and discounts stay out of scope until their rules are decided.
    #[must_use]
    pub fn subtotal_minor(&self) -> MinorUnits {
        self.subtotal_minor
    }

    /// Returns the currency used by the bill, if it has a line.
    #[must_use]
    pub fn currency(&self) -> Option<&str> {
        self.lines.first().map(|line| line.currency.as_str())
    }

    /// Adds `quantity` of `variant_id` to the bill.
    ///
    /// Adding a variant already on the bill increments its quantity. Every rule
    /// in the module documentation is checked before the bill is changed, so a
    /// rejected call leaves it untouched.
    pub fn add(
        &mut self,
        catalog: &Catalog,
        variant_id: &str,
        quantity: u32,
    ) -> Result<(), BillError> {
        if quantity == 0 {
            return Err(BillError::NonPositiveQuantity {
                variant_id: variant_id.to_owned(),
            });
        }

        let variant = catalog
            .variants()
            .iter()
            .find(|variant| variant.id == variant_id)
            .ok_or_else(|| BillError::UnknownVariant(variant_id.to_owned()))?;

        let product_name = catalog
            .products()
            .iter()
            .find(|product| product.id == variant.product_id)
            .map(|product| product.name.clone())
            .ok_or_else(|| BillError::UnknownVariant(variant_id.to_owned()))?;

        let existing = self
            .lines
            .iter()
            .position(|line| line.variant_id == variant_id);
        let requested = match existing {
            Some(index) => self.lines[index].quantity.checked_add(quantity).ok_or(
                BillError::AmountOverflow {
                    variant_id: variant_id.to_owned(),
                },
            )?,
            None => quantity,
        };

        let unit_price_minor = match existing {
            Some(index) => self.lines[index].unit_price_minor,
            None => variant.price_minor,
        };

        let currency = match existing {
            Some(index) => self.lines[index].currency.clone(),
            None => variant.currency.clone(),
        };

        let subtotal = self.validate(
            variant,
            variant_id,
            requested,
            unit_price_minor,
            &currency,
            existing,
        )?;

        let line_total_minor = unit_price_minor
            .checked_mul(i64::from(requested))
            .ok_or_else(|| BillError::AmountOverflow {
                variant_id: variant_id.to_owned(),
            })?;

        match existing {
            Some(index) => {
                let line = &mut self.lines[index];
                line.quantity = requested;
                line.line_total_minor = line_total_minor;
            }
            None => self.lines.push(BillLine {
                variant_id: variant_id.to_owned(),
                sku: variant.sku.clone(),
                product_name,
                unit_price_minor,
                currency,
                quantity: requested,
                line_total_minor,
            }),
        }

        self.subtotal_minor = subtotal;
        Ok(())
    }

    /// Sets the quantity of an existing line, or removes it when `quantity` is
    /// zero.
    ///
    /// Returns [`BillError::UnknownVariant`] when the variant is not on the
    /// bill, so a caller cannot silently set a quantity for an absent line.
    pub fn set_quantity(
        &mut self,
        catalog: &Catalog,
        variant_id: &str,
        quantity: u32,
    ) -> Result<(), BillError> {
        let Some(index) = self
            .lines
            .iter()
            .position(|line| line.variant_id == variant_id)
        else {
            return Err(BillError::UnknownVariant(variant_id.to_owned()));
        };

        if quantity == 0 {
            self.lines.remove(index);
            self.recompute_subtotal();
            return Ok(());
        }

        let variant = catalog
            .variants()
            .iter()
            .find(|variant| variant.id == variant_id)
            .ok_or_else(|| BillError::UnknownVariant(variant_id.to_owned()))?;

        let unit_price_minor = self.lines[index].unit_price_minor;
        let currency = self.lines[index].currency.clone();
        let subtotal = self.validate(
            variant,
            variant_id,
            quantity,
            unit_price_minor,
            &currency,
            Some(index),
        )?;

        let line_total_minor = unit_price_minor
            .checked_mul(i64::from(quantity))
            .ok_or_else(|| BillError::AmountOverflow {
                variant_id: variant_id.to_owned(),
            })?;

        let line = &mut self.lines[index];
        line.quantity = quantity;
        line.line_total_minor = line_total_minor;

        self.subtotal_minor = subtotal;
        Ok(())
    }

    /// Removes `variant_id` from the bill, returning whether it was present.
    pub fn remove(&mut self, variant_id: &str) -> bool {
        let Some(index) = self
            .lines
            .iter()
            .position(|line| line.variant_id == variant_id)
        else {
            return false;
        };
        self.lines.remove(index);
        self.recompute_subtotal();
        true
    }

    /// Removes every line from the bill.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.subtotal_minor = 0;
    }

    /// Validates a prospective line and returns the subtotal it would produce.
    ///
    /// `existing` is the line index being modified, or `None` for a new line.
    /// The existing line is excluded from the currency scan because its own
    /// currency is the one being compared against. Returning the new subtotal
    /// lets the caller store it without a second overflow-prone sum.
    fn validate(
        &self,
        variant: &Variant,
        variant_id: &str,
        quantity: u32,
        unit_price_minor: MinorUnits,
        currency: &str,
        existing: Option<usize>,
    ) -> Result<MinorUnits, BillError> {
        if i64::from(quantity) > variant.stock {
            return Err(BillError::InsufficientStock {
                variant_id: variant_id.to_owned(),
                available: variant.stock,
                requested: quantity,
            });
        }

        if let Some(other) = self
            .lines
            .iter()
            .enumerate()
            .find(|(index, line)| Some(*index) != existing && line.currency != currency)
        {
            return Err(BillError::CurrencyMismatch {
                variant_id: variant_id.to_owned(),
                expected: other.1.currency.clone(),
                found: currency.to_owned(),
            });
        }

        let line_total = unit_price_minor
            .checked_mul(i64::from(quantity))
            .ok_or_else(|| BillError::AmountOverflow {
                variant_id: variant_id.to_owned(),
            })?;

        let mut total: MinorUnits = 0;
        for (index, line) in self.lines.iter().enumerate() {
            let line_total = if Some(index) == existing {
                line_total
            } else {
                line.line_total_minor
            };

            total = total
                .checked_add(line_total)
                .ok_or_else(|| BillError::AmountOverflow {
                    variant_id: variant_id.to_owned(),
                })?;
        }

        if existing.is_none() {
            total = total
                .checked_add(line_total)
                .ok_or_else(|| BillError::AmountOverflow {
                    variant_id: variant_id.to_owned(),
                })?;
        }

        Ok(total)
    }

    /// Recomputes [`Self::subtotal_minor`] from the lines.
    ///
    /// Every line total was already checked for overflow when it was written,
    /// so this stays exact for any bill that could be built.
    fn recompute_subtotal(&mut self) {
        self.subtotal_minor = self.lines.iter().map(|line| line.line_total_minor).sum();
    }
}

/// Formats a bill subtotal for the preview, for example `24.98 USD`.
///
/// The currency is omitted for an empty bill because there is nothing to price
/// yet; the UI renders the same zero either way.
#[must_use]
pub fn format_subtotal(subtotal_minor: MinorUnits, currency: Option<&str>) -> String {
    match currency {
        Some(currency) => crate::catalog::format_price(subtotal_minor, currency),
        None => {
            let units = i128::from(subtotal_minor) / i128::from(MINOR_UNITS_PER_UNIT);
            let minor = i128::from(subtotal_minor) % i128::from(MINOR_UNITS_PER_UNIT);
            format!("{units}.{minor:02}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Bill, BillError, format_subtotal};
    use crate::catalog::{Catalog, MinorUnits, Product, Variant};

    /// Catalog with two in-stock variants and one out-of-stock variant.
    fn catalog() -> Catalog {
        Catalog::new(
            vec![
                Product {
                    id: "p-tee".to_owned(),
                    name: "Cotton T-Shirt".to_owned(),
                    category: "Apparel".to_owned(),
                },
                Product {
                    id: "p-mug".to_owned(),
                    name: "Ceramic Mug".to_owned(),
                    category: "Homeware".to_owned(),
                },
            ],
            vec![
                Variant {
                    id: "v-tee-s".to_owned(),
                    product_id: "p-tee".to_owned(),
                    sku: "TEE-S".to_owned(),
                    price_minor: 1_499,
                    currency: "USD".to_owned(),
                    stock: 5,
                },
                Variant {
                    id: "v-mug".to_owned(),
                    product_id: "p-mug".to_owned(),
                    sku: "MUG-WHT".to_owned(),
                    price_minor: 1_250,
                    currency: "USD".to_owned(),
                    stock: 2,
                },
                Variant {
                    id: "v-tee-l".to_owned(),
                    product_id: "p-tee".to_owned(),
                    sku: "TEE-L".to_owned(),
                    price_minor: 1_599,
                    currency: "USD".to_owned(),
                    stock: 0,
                },
            ],
        )
    }

    #[test]
    fn a_new_bill_is_empty() {
        let bill = Bill::new();
        assert!(bill.is_empty());
        assert_eq!(bill.len(), 0);
        assert_eq!(bill.subtotal_minor(), 0);
        assert_eq!(bill.currency(), None);
    }

    #[test]
    fn adding_a_line_copies_catalog_values_and_totals() {
        let catalog = catalog();
        let mut bill = Bill::new();

        bill.add(&catalog, "v-tee-s", 2).expect("add");

        assert_eq!(bill.len(), 1);
        let line = &bill.lines()[0];
        assert_eq!(line.sku, "TEE-S");
        assert_eq!(line.product_name, "Cotton T-Shirt");
        assert_eq!(line.unit_price_minor, 1_499);
        assert_eq!(line.currency, "USD");
        assert_eq!(line.quantity, 2);
        assert_eq!(line.line_total_minor, 2_998);
        assert_eq!(bill.subtotal_minor(), 2_998);
        assert_eq!(bill.currency(), Some("USD"));
    }

    #[test]
    fn adding_the_same_variant_merges_quantities() {
        let catalog = catalog();
        let mut bill = Bill::new();

        bill.add(&catalog, "v-tee-s", 2).expect("add");
        bill.add(&catalog, "v-tee-s", 1).expect("add again");

        assert_eq!(bill.len(), 1, "one line per variant");
        assert_eq!(bill.lines()[0].quantity, 3);
        assert_eq!(bill.lines()[0].line_total_minor, 4_497);
        assert_eq!(bill.subtotal_minor(), 4_497);
    }

    #[test]
    fn multiple_lines_sum_exactly() {
        let catalog = catalog();
        let mut bill = Bill::new();

        bill.add(&catalog, "v-tee-s", 2).expect("add tee");
        bill.add(&catalog, "v-mug", 1).expect("add mug");

        assert_eq!(bill.len(), 2);
        assert_eq!(
            bill.subtotal_minor(),
            1_499 * 2 + 1_250,
            "subtotal must be exact integer minor units"
        );
    }

    #[test]
    fn a_zero_quantity_is_rejected_and_changes_nothing() {
        let catalog = catalog();
        let mut bill = Bill::new();

        let error = bill.add(&catalog, "v-tee-s", 0).expect_err("must reject");

        assert_eq!(
            error,
            BillError::NonPositiveQuantity {
                variant_id: "v-tee-s".to_owned()
            }
        );
        assert!(bill.is_empty());
        assert_eq!(bill.subtotal_minor(), 0);
    }

    #[test]
    fn an_unknown_variant_is_rejected() {
        let catalog = catalog();
        let mut bill = Bill::new();

        let error = bill
            .add(&catalog, "does-not-exist", 1)
            .expect_err("must reject");

        assert_eq!(
            error,
            BillError::UnknownVariant("does-not-exist".to_owned())
        );
        assert!(bill.is_empty());
    }

    #[test]
    fn an_out_of_stock_variant_cannot_be_added() {
        let catalog = catalog();
        let mut bill = Bill::new();

        let error = bill.add(&catalog, "v-tee-l", 1).expect_err("must reject");

        assert_eq!(
            error,
            BillError::InsufficientStock {
                variant_id: "v-tee-l".to_owned(),
                available: 0,
                requested: 1,
            }
        );
        assert!(bill.is_empty());
    }

    #[test]
    fn a_quantity_above_available_stock_is_rejected() {
        let catalog = catalog();
        let mut bill = Bill::new();

        let error = bill.add(&catalog, "v-mug", 3).expect_err("must reject");

        assert_eq!(
            error,
            BillError::InsufficientStock {
                variant_id: "v-mug".to_owned(),
                available: 2,
                requested: 3,
            }
        );
        assert!(bill.is_empty());
    }

    #[test]
    fn a_merged_quantity_cannot_exceed_stock() {
        let catalog = catalog();
        let mut bill = Bill::new();

        bill.add(&catalog, "v-mug", 2).expect("add all stock");

        let error = bill.add(&catalog, "v-mug", 1).expect_err("must reject");

        assert_eq!(
            error,
            BillError::InsufficientStock {
                variant_id: "v-mug".to_owned(),
                available: 2,
                requested: 3,
            }
        );
        assert_eq!(
            bill.lines()[0].quantity,
            2,
            "the existing line is unchanged"
        );
        assert_eq!(bill.subtotal_minor(), 2_500);
    }

    #[test]
    fn setting_a_quantity_updates_the_line_and_subtotal() {
        let catalog = catalog();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-tee-s", 1).expect("add");

        bill.set_quantity(&catalog, "v-tee-s", 4).expect("set");

        assert_eq!(bill.lines()[0].quantity, 4);
        assert_eq!(bill.lines()[0].line_total_minor, 1_499 * 4);
        assert_eq!(bill.subtotal_minor(), 1_499 * 4);
    }

    #[test]
    fn setting_a_zero_quantity_removes_the_line() {
        let catalog = catalog();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-tee-s", 2).expect("add");

        bill.set_quantity(&catalog, "v-tee-s", 0).expect("remove");

        assert!(bill.is_empty());
        assert_eq!(bill.subtotal_minor(), 0);
    }

    #[test]
    fn setting_a_quantity_above_stock_is_rejected() {
        let catalog = catalog();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-mug", 1).expect("add");

        let error = bill
            .set_quantity(&catalog, "v-mug", 3)
            .expect_err("must reject");

        assert_eq!(
            error,
            BillError::InsufficientStock {
                variant_id: "v-mug".to_owned(),
                available: 2,
                requested: 3,
            }
        );
        assert_eq!(bill.lines()[0].quantity, 1);
    }

    #[test]
    fn setting_a_quantity_for_an_absent_line_is_rejected() {
        let catalog = catalog();
        let mut bill = Bill::new();

        let error = bill
            .set_quantity(&catalog, "v-tee-s", 1)
            .expect_err("must reject");

        assert_eq!(error, BillError::UnknownVariant("v-tee-s".to_owned()));
    }

    #[test]
    fn removing_a_line_updates_the_subtotal() {
        let catalog = catalog();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-tee-s", 2).expect("add tee");
        bill.add(&catalog, "v-mug", 1).expect("add mug");

        assert!(bill.remove("v-tee-s"));

        assert_eq!(bill.len(), 1);
        assert_eq!(bill.subtotal_minor(), 1_250);
        assert!(!bill.remove("v-tee-s"), "removing again is a no-op");
    }

    #[test]
    fn clearing_the_bill_empties_it() {
        let catalog = catalog();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-tee-s", 2).expect("add");

        bill.clear();

        assert!(bill.is_empty());
        assert_eq!(bill.subtotal_minor(), 0);
    }

    #[test]
    fn a_mixed_currency_bill_is_rejected() {
        let catalog = Catalog::new(
            vec![Product {
                id: "p".to_owned(),
                name: "Mixed".to_owned(),
                category: "Test".to_owned(),
            }],
            vec![
                Variant {
                    id: "v-usd".to_owned(),
                    product_id: "p".to_owned(),
                    sku: "USD".to_owned(),
                    price_minor: 100,
                    currency: "USD".to_owned(),
                    stock: 5,
                },
                Variant {
                    id: "v-eur".to_owned(),
                    product_id: "p".to_owned(),
                    sku: "EUR".to_owned(),
                    price_minor: 100,
                    currency: "EUR".to_owned(),
                    stock: 5,
                },
            ],
        );
        let mut bill = Bill::new();
        bill.add(&catalog, "v-usd", 1).expect("add usd");

        let error = bill.add(&catalog, "v-eur", 1).expect_err("must reject");

        assert_eq!(
            error,
            BillError::CurrencyMismatch {
                variant_id: "v-eur".to_owned(),
                expected: "USD".to_owned(),
                found: "EUR".to_owned(),
            }
        );
        assert_eq!(bill.len(), 1);
    }

    #[test]
    fn an_amount_overflow_is_rejected() {
        let catalog = Catalog::new(
            vec![Product {
                id: "p".to_owned(),
                name: "Pricey".to_owned(),
                category: "Test".to_owned(),
            }],
            vec![Variant {
                id: "v".to_owned(),
                product_id: "p".to_owned(),
                sku: "SKU".to_owned(),
                price_minor: i64::MAX,
                currency: "USD".to_owned(),
                stock: 2,
            }],
        );
        let mut bill = Bill::new();

        let error: BillError = bill.add(&catalog, "v", 2).expect_err("must reject");

        assert_eq!(
            error,
            BillError::AmountOverflow {
                variant_id: "v".to_owned()
            }
        );
        assert!(bill.is_empty());
    }

    #[test]
    fn subtotal_formatting_shows_the_currency_when_known() {
        assert_eq!(format_subtotal(2_998, Some("USD")), "29.98 USD");
        assert_eq!(format_subtotal(0, None), "0.00");
        assert_eq!(format_subtotal(5, None), "0.05");
        assert_eq!(
            format_subtotal(0, Some("USD")),
            "0.00 USD",
            "an empty USD bill still names its currency once a line exists"
        );
    }

    #[test]
    fn subtotal_is_zero_after_removing_every_line() {
        let catalog = catalog();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-tee-s", 1).expect("add");
        bill.add(&catalog, "v-mug", 1).expect("add");

        bill.remove("v-tee-s");
        bill.remove("v-mug");

        assert!(bill.is_empty());
        assert_eq!(bill.subtotal_minor(), 0);
        assert_eq!(bill.currency(), None);
    }

    #[test]
    fn bill_lines_keep_their_price_after_the_catalog_changes() {
        let mut catalog = catalog();
        let mut bill = Bill::new();
        bill.add(&catalog, "v-tee-s", 2).expect("add");

        // Simulate a catalog refresh with a new price.
        let variants: Vec<Variant> = catalog
            .variants()
            .iter()
            .cloned()
            .map(|mut variant| {
                if variant.id == "v-tee-s" {
                    variant.price_minor = 999_999;
                }
                variant
            })
            .collect();
        let products = catalog.products().to_vec();
        catalog = Catalog::new(products, variants);

        assert_eq!(
            bill.lines()[0].unit_price_minor,
            1_499,
            "the line keeps the price shown when it was added"
        );
        assert_eq!(bill.subtotal_minor(), 2_998);

        // Checkout re-reads the catalog, so a fresh add sees the new price.
        let mut refreshed = Bill::new();
        refreshed.add(&catalog, "v-tee-s", 1).expect("add");
        assert_eq!(refreshed.lines()[0].unit_price_minor, 999_999);
    }

    #[test]
    fn a_large_but_valid_bill_stays_exact() {
        let catalog = catalog();
        let mut bill = Bill::new();

        bill.add(&catalog, "v-tee-s", 5).expect("all tee stock");
        bill.add(&catalog, "v-mug", 2).expect("all mug stock");

        let expected: MinorUnits = 1_499 * 5 + 1_250 * 2;
        assert_eq!(bill.subtotal_minor(), expected);
        assert_eq!(bill.len(), 2);
    }
}
