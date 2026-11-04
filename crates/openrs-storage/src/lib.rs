//! Storage layer for OpenRS.
//!
//! This crate owns the persistence contract and the SQLite schema. It is shared
//! by both targets: `rusqlite` on the desktop, `sqlite-wasm-rs` + OPFS in the
//! browser. Neither shell nor `openrs-ui` contains SQL.
//!
//! # The guarantees this layer must provide
//!
//! `SPEC.md` promises:
//!
//! 1. A failed checkout leaves no partial sale and no incorrect stock count.
//! 2. Retrying the same checkout records the sale and deducts stock only once.
//! 3. A recorded sale keeps the prices used at checkout forever.
//!
//! Every implementation of [`Repository`] is tested against the same guarantee
//! suite, natively and in a browser. See `tests/guarantees.rs`.
//!
//! # Layout
//!
//! - [`model`] — value types, errors, and the injectable [`model::Clock`] and
//!   [`model::IdSource`] services. No platform calls.
//! - [`memory`] — the behavioral reference implementation.
//! - [`sqlite`] — the production backend. The SQL logic is written once against
//!   a small driver trait; `sqlite/native.rs` and `sqlite/wasm.rs` are the only
//!   platform-specific files.
//!
//! # Platform differences (ADR 0002)
//!
//! - Native uses WAL with `synchronous = FULL`. The browser OPFS backend uses
//!   the rollback journal because OPFS does not support WAL.
//! - The browser database must live in a dedicated worker, so callers on web
//!   reach [`SqliteRepository::open_opfs`] through an async worker adapter.
//! - Only [`sqlite`] is allowed to use `unsafe` (raw SQLite FFI on web). The
//!   rest of the workspace keeps `unsafe_code = "forbid"`.

#![warn(missing_docs)]

pub mod memory;
pub mod model;
pub mod sqlite;

pub use memory::MemoryRepository;
pub use model::{
    Catalog, CheckoutLine, CheckoutOutcome, CheckoutReceipt, CheckoutRequest, Clock, IdSource,
    MinorUnits, Product, RepoError, Sale, SaleLine, SeedOutcome, Variant,
};
pub use sqlite::SqliteRepository;

/// Persistence contract shared by every storage backend.
///
/// The trait is intentionally narrow: it exposes the operations the sale
/// workflows need, and nothing about how they are stored.
pub trait Repository {
    /// Inserts a product.
    fn add_product(&mut self, product: &Product) -> Result<(), RepoError>;

    /// Loads the whole catalog snapshot, in the deterministic order
    /// [`Catalog::new`] applies.
    ///
    /// Only active products and their variants are loaded. Inactive products
    /// are hidden from the UI until catalog management can reactivate them.
    fn load_catalog(&self) -> Result<Catalog, RepoError>;

    /// Writes `seed` when the catalog is empty, and does nothing otherwise.
    ///
    /// This is how a fresh installation gets its starting catalog. It must be
    /// all-or-nothing, so a seed catalog with a variant whose product is
    /// missing leaves the database unchanged and returns
    /// [`RepoError::UnknownProduct`].
    fn seed_catalog_if_empty(&mut self, seed: &Catalog) -> Result<SeedOutcome, RepoError>;

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
    ///
    /// A request with no lines is rejected with [`RepoError::EmptyCheckout`]
    /// before anything is written, and its key stays unused.
    fn checkout(&mut self, request: &CheckoutRequest) -> Result<CheckoutOutcome, RepoError>;

    /// Looks up a sale by its idempotency key.
    fn sale_by_checkout(&self, checkout_id: &str) -> Result<Option<Sale>, RepoError>;

    /// Returns how many sales are recorded. Used by tests to detect duplicates.
    fn sale_count(&self) -> Result<i64, RepoError>;
}

/// Seeds `repository` when it is empty, then returns the catalog it holds.
///
/// Both shells call this once at startup, so desktop and web run the same
/// bootstrap sequence. The first call on a fresh database writes `seed`; every
/// later call, and every call on a database with products, only reads.
///
/// ```no_run
/// # use std::sync::Arc;
/// # use openrs_storage::{Catalog, Repository, SeedOutcome};
/// # fn example(repository: &mut dyn Repository) -> Result<(), openrs_storage::RepoError> {
/// let outcome = repository.seed_catalog_if_empty(&Catalog::sample())?;
/// let catalog = repository.load_catalog()?;
/// # let _ = (outcome, catalog);
/// # Ok(())
/// # }
/// ```
pub fn load_or_seed_catalog(
    repository: &mut dyn Repository,
    seed: &Catalog,
) -> Result<(SeedOutcome, Catalog), RepoError> {
    let outcome = repository.seed_catalog_if_empty(seed)?;
    let catalog = repository.load_catalog()?;
    Ok((outcome, catalog))
}
