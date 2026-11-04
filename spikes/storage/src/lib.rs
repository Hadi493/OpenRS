//! Storage spike for OpenRS.
//!
//! This crate is **not** product code. It exists to answer one question with
//! evidence: can a single SQLite schema plus a single transaction model satisfy
//! the sale guarantees in `SPEC.md` on both the native and the WebAssembly
//! target?
//!
//! To answer it, the crate defines one [`Repository`] trait with two
//! implementations — [`memory::MemoryRepo`] and [`sqlite::SqliteRepo`] — and
//! runs the same guarantee suite against both.
//!
//! Money is represented as integer minor units plus a currency code. There is
//! no floating point anywhere in this crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod memory;
pub mod model;
pub mod sqlite;

pub use model::{
    CheckoutLine, CheckoutOutcome, CheckoutRequest, Clock, IdSource, MinorUnits, Product,
    RepoError, Sale, SaleLine, Variant,
};

/// Persistence contract that both candidate backends must satisfy.
///
/// The trait is intentionally narrow: it only exposes the operations needed to
/// prove the `SPEC.md` guarantees. It is not a finished repository design.
pub trait Repository {
    /// Inserts a product.
    fn add_product(&mut self, product: &Product) -> Result<(), RepoError>;

    /// Inserts a sellable variant belonging to an existing product.
    fn add_variant(&mut self, variant: &Variant) -> Result<(), RepoError>;

    /// Returns the current stock for a variant.
    fn stock_of(&self, variant_id: &str) -> Result<i64, RepoError>;

    /// Changes a variant's unit price, as a catalog edit would.
    fn set_price(&mut self, variant_id: &str, price_minor: MinorUnits) -> Result<(), RepoError>;

    /// Completes a checkout atomically.
    ///
    /// Implementations must guarantee all three `SPEC.md` properties:
    ///
    /// 1. Either the sale, its lines, and the stock deduction are all persisted,
    ///    or none of them are.
    /// 2. A repeated call with the same [`CheckoutRequest::checkout_id`] returns
    ///    the previously recorded sale and never deducts stock again.
    /// 3. The recorded sale keeps the unit prices from the moment of checkout.
    fn checkout(&mut self, request: &CheckoutRequest) -> Result<CheckoutOutcome, RepoError>;

    /// Looks up a sale by its idempotency key.
    fn sale_by_checkout(&self, checkout_id: &str) -> Result<Option<Sale>, RepoError>;

    /// Returns how many sales are recorded. Used by tests to detect duplicates.
    fn sale_count(&self) -> Result<i64, RepoError>;
}
