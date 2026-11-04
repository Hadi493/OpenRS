//! Value types, the domain error type, and the injectable services.
//!
//! These types are shared by every storage backend and by the UI. They contain
//! no SQL, no platform calls, and no floating-point money.
//!
//! The catalog types ([`Product`], [`Variant`], [`MinorUnits`]) and the
//! checkout request shapes ([`CheckoutLine`], [`CheckoutRequest`],
//! [`CheckoutReceipt`]) are defined in `openrs-core` and re-exported here, so
//! the storage layer, the UI, and the worker protocol cannot drift apart. The
//! recorded [`Sale`] itself still lives in this crate: only the storage layer
//! reads one today, and `openrs-ui` deliberately does not depend on storage.

use std::fmt;

pub use openrs_core::{
    Catalog, CheckoutLine, CheckoutReceipt, CheckoutRequest, MinorUnits, Product, Variant,
};

/// What [`crate::Repository::seed_catalog_if_empty`] did.
///
/// The distinction matters to the shells: [`Self::Seeded`] means the first run
/// wrote the development sample catalog, [`Self::AlreadyPopulated`] means the
/// existing catalog was left untouched. Tests assert on the variant instead of a
/// bare `bool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedOutcome {
    /// The seed catalog was written; the database had no products.
    Seeded,
    /// The database already had products, so nothing was written.
    AlreadyPopulated,
}

// `CheckoutLine`, `CheckoutRequest`, and `CheckoutReceipt` are re-exported
// above: the UI builds requests and shows receipts, so those shapes live in
// `openrs-core` next to `Bill` and cannot drift from it.

/// The result of a checkout call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutOutcome {
    /// The sale was recorded by this call.
    Completed(Sale),
    /// The same `checkout_id` had already been recorded; nothing changed.
    Replayed(Sale),
}

/// A recorded sale, including the historical snapshot of every line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sale {
    /// Sale identifier.
    pub id: String,
    /// Idempotency key that produced this sale.
    pub checkout_id: String,
    /// Creation timestamp, RFC 3339 in UTC.
    pub created_at: String,
    /// Currency shared by every line in this sale.
    pub currency: String,
    /// Sum of the line totals.
    pub subtotal_minor: MinorUnits,
    /// Final total. Equal to the subtotal while tax and discounts are out of
    /// scope.
    pub total_minor: MinorUnits,
    /// Immutable line snapshot.
    pub lines: Vec<SaleLine>,
}

impl Sale {
    /// Returns the quantity sold for `variant_id`, if it appears in the sale.
    #[must_use]
    pub fn quantity_of(&self, variant_id: &str) -> u32 {
        self.lines
            .iter()
            .filter(|line| line.variant_id == variant_id)
            .map(|line| line.quantity)
            .sum()
    }
}

/// An immutable snapshot of one sold line.
///
/// The name, SKU, and unit price are copied at checkout time. They are **not**
/// resolved by joining back to `variants`, which is what makes a recorded sale
/// immune to later catalog edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleLine {
    /// Variant that was sold.
    pub variant_id: String,
    /// SKU at the time of sale.
    pub sku: String,
    /// Product name at the time of sale.
    pub product_name: String,
    /// Unit price at the time of sale.
    pub unit_price_minor: MinorUnits,
    /// Currency at the time of sale.
    pub currency: String,
    /// Quantity sold.
    pub quantity: u32,
    /// `unit_price_minor * quantity`, precomputed so history never recomputes.
    pub line_total_minor: MinorUnits,
}

/// Why a storage operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoError {
    /// The variant does not exist.
    UnknownVariant(String),
    /// A checkout with no lines was requested.
    ///
    /// Recording a zero-line sale would be a meaningless row that still
    /// consumes an idempotency key, so it is rejected before any write.
    EmptyCheckout,
    /// A quantity of zero was requested.
    NonPositiveQuantity {
        /// Offending variant.
        variant_id: String,
    },
    /// The requested quantity exceeds available stock.
    InsufficientStock {
        /// Offending variant.
        variant_id: String,
        /// Stock available at the time of the attempt.
        available: i64,
        /// Quantity the checkout asked for.
        requested: u32,
    },
    /// A variant referenced a product that does not exist.
    UnknownProduct(String),
    /// One sale mixed two currencies, which is not supported.
    CurrencyMismatch {
        /// Offending variant.
        variant_id: String,
        /// Currency already used by the sale.
        expected: String,
        /// Currency the offending variant uses.
        found: String,
    },
    /// A quantity times a unit price, or the running subtotal, overflowed.
    AmountOverflow {
        /// Offending variant.
        variant_id: String,
    },
    /// The backend rejected the write. The string is diagnostic only.
    Storage(String),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownVariant(id) => write!(f, "unknown variant `{id}`"),
            Self::EmptyCheckout => write!(f, "a checkout must contain at least one line"),
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
            Self::UnknownProduct(id) => write!(f, "unknown product `{id}`"),
            Self::CurrencyMismatch {
                variant_id,
                expected,
                found,
            } => write!(
                f,
                "variant `{variant_id}` uses currency `{found}` but the sale already uses `{expected}`"
            ),
            Self::AmountOverflow { variant_id } => {
                write!(f, "amount overflow while pricing `{variant_id}`")
            }
            Self::Storage(message) => write!(f, "storage error: {message}"),
        }
    }
}

impl std::error::Error for RepoError {}

/// Source of the current time.
///
/// Injected so tests are deterministic and so the WASM target, which has no
/// guaranteed system clock exposure, can supply a browser clock.
pub trait Clock: Send + Sync {
    /// Returns the current UTC time as an RFC 3339 string.
    fn now_rfc3339(&self) -> String;
}

/// Source of sale identifiers.
///
/// Injected for the same reason as [`Clock`], and because the native concurrency
/// tests proved that per-process sequences collide across connections. The
/// product uses UUIDv7 values (ADR 0002).
pub trait IdSource: Send + Sync {
    /// Returns a new, previously unused sale identifier.
    fn next_sale_id(&self) -> String;
}
