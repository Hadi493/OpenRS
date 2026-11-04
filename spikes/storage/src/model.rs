//! Value types and the error type shared by every storage backend.

use std::fmt;

/// Exact money amount in integer minor units (for example, cents).
///
/// `SPEC.md` forbids floating point for money, so every amount in this crate is
/// an `i64` count of minor units paired with an ISO-4217-style currency code.
pub type MinorUnits = i64;

/// A product, matching the `Product` section of `SPEC.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    /// Stable product identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Flat category label. Hierarchy is not part of the spike.
    pub category: String,
}

/// A sellable variant of a product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// Stable variant identifier.
    pub id: String,
    /// Owning product identifier.
    pub product_id: String,
    /// SKU or barcode. Unique when present.
    pub sku: String,
    /// Unit price in minor units.
    pub price_minor: MinorUnits,
    /// Currency code for [`Self::price_minor`].
    pub currency: String,
    /// Available stock. Must never go negative.
    pub stock: i64,
}

/// One requested line in a checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutLine {
    /// Variant being sold.
    pub variant_id: String,
    /// Quantity to sell. Must be greater than zero.
    pub quantity: u32,
}

/// A checkout attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutRequest {
    /// Idempotency key.
    ///
    /// The caller generates one key per *checkout attempt* and reuses it on
    /// retry. The key must be stable across a network retry or a double click.
    pub checkout_id: String,
    /// Lines to sell.
    pub lines: Vec<CheckoutLine>,
}

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
    /// Final total. Equal to the subtotal in the spike because tax and
    /// discounts are explicitly out of scope.
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

/// Why a repository operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoError {
    /// The variant does not exist.
    UnknownVariant(String),
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
    /// The backend rejected the write. The string is diagnostic only.
    Storage(String),
}

impl fmt::Display for RepoError {
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
            Self::UnknownProduct(id) => write!(f, "unknown product `{id}`"),
            Self::Storage(message) => write!(f, "storage error: {message}"),
        }
    }
}

impl std::error::Error for RepoError {}

impl From<rusqlite::Error> for RepoError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

/// Source of the current time.
///
/// Injected so tests are deterministic and so the WASM target, which has no
/// `SystemTime` guarantee, can supply a browser clock later.
pub trait Clock: Send + Sync {
    /// Returns the current UTC time as an RFC 3339 string.
    fn now_rfc3339(&self) -> String;
}

/// Source of sale identifiers.
///
/// Injected for the same reason as [`Clock`]: a future WASM build must supply
/// its own randomness source, and tests need deterministic ids.
pub trait IdSource: Send + Sync {
    /// Returns a new, previously unused sale identifier.
    fn next_sale_id(&self) -> String;
}
