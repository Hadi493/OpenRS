//! SQLite backend, used on both targets.
//!
//! # How each `SPEC.md` guarantee is enforced
//!
//! | Guarantee | Mechanism |
//! |---|---|
//! | No partial sale, no wrong stock | One `BEGIN IMMEDIATE` transaction covers the sale row, the line rows, and the stock decrements. Any error rolls the whole transaction back. |
//! | Retry must not double-record | `sales.checkout_id` is `UNIQUE`; the retry path reads the existing sale inside the same transaction and returns it unchanged. |
//! | Sale keeps its prices | `sale_items` copies `unit_price_minor`, `sku`, and `product_name`. Reading a sale never joins to `variants`. |
//! | Stock can never go negative | `CHECK (stock >= 0)` in the schema plus a guarded `UPDATE ... WHERE stock >= ?`. |
//!
//! The SQL statements live here and are executed through the small
//! [`driver::SqlSession`] surface. `native` and `wasm` only implement that
//! surface, so the checkout logic exists once.

mod driver;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native as platform;

#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm as platform;

use std::sync::Arc;

use self::driver::{Param, Row, SqlSession};
use crate::Repository;
use crate::model::{
    Catalog, CheckoutLine, CheckoutOutcome, CheckoutRequest, Clock, IdSource, MinorUnits, Product,
    RepoError, Sale, SaleLine, SeedOutcome, Variant,
};

const SCHEMA: &str = include_str!("../../migrations/0001_init.sql");

/// Schema version stored in `PRAGMA user_version`.
///
/// A migration adds a `000N_*.sql` file, bumps this number, and applies only the
/// missing steps. There is intentionally no migration framework yet.
const SCHEMA_VERSION: i64 = 1;

/// SQLite-backed [`Repository`].
///
/// On the desktop this opens a normal database file. In the browser it is
/// created with [`SqliteRepository::open_opfs`] inside a dedicated worker.
pub struct SqliteRepository {
    db: platform::Db,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
}

impl SqliteRepository {
    /// Opens an in-memory database and applies the schema.
    ///
    /// Used by tests on both targets. The data disappears when the repository
    /// is dropped, which is deliberate: it makes the guarantee suite identical
    /// on desktop and web.
    pub fn open_in_memory(
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Result<Self, RepoError> {
        Self::from_db(platform::Db::open_in_memory()?, clock, ids)
    }

    /// Opens (or creates) a database file and applies the schema.
    ///
    /// Native only: the browser has no file system path. Web callers use
    /// [`Self::open_opfs`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(
        path: impl AsRef<std::path::Path>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Result<Self, RepoError> {
        Self::from_db(platform::Db::open(path.as_ref())?, clock, ids)
    }

    /// Installs the OPFS VFS and opens a database inside it.
    ///
    /// Must be awaited: installing the VFS talks to the browser file system
    /// through promises. OPFS sync access handles only exist in a dedicated
    /// worker, so this is meant to be called from the worker that owns the
    /// database, never from the UI thread.
    ///
    /// `name` identifies the database file inside the OPFS pool directory.
    #[cfg(target_arch = "wasm32")]
    pub async fn open_opfs(
        name: &str,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Result<Self, RepoError> {
        platform::install_opfs_vfs().await?;
        Self::from_db(platform::Db::open(name)?, clock, ids)
    }

    fn from_db(
        mut db: platform::Db,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Result<Self, RepoError> {
        migrate(&mut db)?;
        Ok(Self { db, clock, ids })
    }
}

/// Applies any missing migration.
///
/// The version is read *inside* the `IMMEDIATE` transaction. Reading it first
/// and then migrating is a race: two connections opening a fresh database could
/// both see version 0 and both try to create the tables.
fn migrate(db: &mut platform::Db) -> Result<(), RepoError> {
    db.transaction(|session: &dyn SqlSession| {
        let version = match session.query_row("PRAGMA user_version", &[])? {
            Some(row) => row.integer(0)?,
            None => 0,
        };

        if version < 1 {
            session.execute_batch(SCHEMA)?;
        }

        // A version bump above 1 would apply the missing steps here.
        session.execute(&format!("PRAGMA user_version = {SCHEMA_VERSION}"), &[])?;
        Ok(())
    })
}

fn product_exists(session: &dyn SqlSession, product_id: &str) -> Result<bool, RepoError> {
    Ok(session
        .query_row(
            "SELECT 1 FROM products WHERE id = ?1",
            &[Param::Text(product_id)],
        )?
        .is_some())
}

fn insert_product(session: &dyn SqlSession, product: &Product) -> Result<(), RepoError> {
    session.execute(
        "INSERT INTO products (id, name, category, active) VALUES (?1, ?2, ?3, 1)",
        &[
            Param::Text(&product.id),
            Param::Text(&product.name),
            Param::Text(&product.category),
        ],
    )?;
    Ok(())
}

fn insert_variant(session: &dyn SqlSession, variant: &Variant) -> Result<(), RepoError> {
    session.execute(
        "INSERT INTO variants (id, product_id, sku, price_minor, currency, stock)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        &[
            Param::Text(&variant.id),
            Param::Text(&variant.product_id),
            Param::Text(&variant.sku),
            Param::Integer(variant.price_minor),
            Param::Text(&variant.currency),
            Param::Integer(variant.stock),
        ],
    )?;
    Ok(())
}

/// Reads the whole catalog. Only active products (and their variants) are
/// returned; inactive products are filtered in SQL so the UI cannot list a
/// product it is not allowed to sell.
fn load_catalog_from(session: &dyn SqlSession) -> Result<Catalog, RepoError> {
    let products = session
        .query_rows(
            "SELECT id, name, category FROM products WHERE active = 1",
            &[],
        )?
        .into_iter()
        .map(|row| {
            Ok(Product {
                id: row.text(0)?,
                name: row.text(1)?,
                category: row.text(2)?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;

    let variants = session
        .query_rows(
            "SELECT v.id, v.product_id, v.sku, v.price_minor, v.currency, v.stock
             FROM variants v
             JOIN products p ON p.id = v.product_id
             WHERE p.active = 1",
            &[],
        )?
        .into_iter()
        .map(|row| {
            Ok(Variant {
                id: row.text(0)?,
                product_id: row.text(1)?,
                sku: row.text(2)?,
                price_minor: row.integer(3)?,
                currency: row.text(4)?,
                stock: row.integer(5)?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;

    Ok(Catalog::new(products, variants))
}

/// Writes `seed` only when no product exists, all inside the caller's
/// transaction so a bad seed leaves no rows behind.
fn seed_in(session: &dyn SqlSession, seed: &Catalog) -> Result<SeedOutcome, RepoError> {
    let count = session
        .query_row("SELECT COUNT(*) FROM products", &[])?
        .ok_or_else(|| RepoError::Storage("COUNT returned no row".to_owned()))?
        .integer(0)?;

    if count > 0 {
        return Ok(SeedOutcome::AlreadyPopulated);
    }

    for product in seed.products() {
        insert_product(session, product)?;
    }

    for variant in seed.variants() {
        if !product_exists(session, &variant.product_id)? {
            return Err(RepoError::UnknownProduct(variant.product_id.clone()));
        }
        insert_variant(session, variant)?;
    }

    Ok(SeedOutcome::Seeded)
}

fn load_sale_by_checkout(
    session: &dyn SqlSession,
    checkout_id: &str,
) -> Result<Option<Sale>, RepoError> {
    let row = session.query_row(
        "SELECT id FROM sales WHERE checkout_id = ?1",
        &[Param::Text(checkout_id)],
    )?;

    match row {
        Some(row) => load_sale(session, &row.text(0)?).map(Some),
        None => Ok(None),
    }
}

fn load_sale(session: &dyn SqlSession, sale_id: &str) -> Result<Sale, RepoError> {
    let row = session
        .query_row(
            "SELECT id, checkout_id, created_at, currency, subtotal_minor, total_minor
             FROM sales WHERE id = ?1",
            &[Param::Text(sale_id)],
        )?
        .ok_or_else(|| {
            RepoError::Storage(format!("sale `{sale_id}` disappeared mid-transaction"))
        })?;

    let lines = session
        .query_rows(
            "SELECT variant_id, sku, product_name, unit_price_minor, currency, quantity,
                    line_total_minor
             FROM sale_items WHERE sale_id = ?1 ORDER BY id",
            &[Param::Text(sale_id)],
        )?
        .into_iter()
        .map(sale_line_from_row)
        .collect::<Result<Vec<_>, RepoError>>()?;

    Ok(Sale {
        id: row.text(0)?,
        checkout_id: row.text(1)?,
        created_at: row.text(2)?,
        currency: row.text(3)?,
        subtotal_minor: row.integer(4)?,
        total_minor: row.integer(5)?,
        lines,
    })
}

fn sale_line_from_row(row: Row) -> Result<SaleLine, RepoError> {
    let quantity = row.integer(5)?;
    Ok(SaleLine {
        variant_id: row.text(0)?,
        sku: row.text(1)?,
        product_name: row.text(2)?,
        unit_price_minor: row.integer(3)?,
        currency: row.text(4)?,
        quantity: u32::try_from(quantity)
            .map_err(|_| RepoError::Storage(format!("invalid stored quantity {quantity}")))?,
        line_total_minor: row.integer(6)?,
    })
}

/// Prices and validates one checkout line without writing anything.
fn price_line(
    session: &dyn SqlSession,
    line: &CheckoutLine,
    currency: &mut Option<String>,
) -> Result<SaleLine, RepoError> {
    if line.quantity == 0 {
        return Err(RepoError::NonPositiveQuantity {
            variant_id: line.variant_id.clone(),
        });
    }

    let row = session
        .query_row(
            "SELECT v.sku, v.price_minor, v.currency, v.stock, p.name
             FROM variants v
             JOIN products p ON p.id = v.product_id
             WHERE v.id = ?1",
            &[Param::Text(&line.variant_id)],
        )?
        .ok_or_else(|| RepoError::UnknownVariant(line.variant_id.clone()))?;

    let sku = row.text(0)?;
    let price_minor = row.integer(1)?;
    let line_currency = row.text(2)?;
    let stock = row.integer(3)?;
    let product_name = row.text(4)?;

    if i64::from(line.quantity) > stock {
        return Err(RepoError::InsufficientStock {
            variant_id: line.variant_id.clone(),
            available: stock,
            requested: line.quantity,
        });
    }

    match currency {
        Some(expected) if expected != &line_currency => {
            return Err(RepoError::CurrencyMismatch {
                variant_id: line.variant_id.clone(),
                expected: expected.clone(),
                found: line_currency,
            });
        }
        Some(_) => {}
        None => *currency = Some(line_currency.clone()),
    }

    let line_total = price_minor
        .checked_mul(i64::from(line.quantity))
        .ok_or_else(|| RepoError::AmountOverflow {
            variant_id: line.variant_id.clone(),
        })?;

    Ok(SaleLine {
        variant_id: line.variant_id.clone(),
        sku,
        product_name,
        unit_price_minor: price_minor,
        currency: line_currency,
        quantity: line.quantity,
        line_total_minor: line_total,
    })
}

/// The checkout algorithm, shared by both platforms.
///
/// Runs entirely inside one transaction owned by the driver. Every early return
/// leaves the database untouched.
fn checkout_in(
    session: &dyn SqlSession,
    request: &CheckoutRequest,
    clock: &dyn Clock,
    ids: &dyn IdSource,
) -> Result<CheckoutOutcome, RepoError> {
    // Retry path: the same checkout key returns the recorded sale and changes
    // nothing. Checked inside the transaction so a concurrent duplicate cannot
    // slip between the read and the write.
    if let Some(existing) = load_sale_by_checkout(session, &request.checkout_id)? {
        return Ok(CheckoutOutcome::Replayed(existing));
    }

    if request.lines.is_empty() {
        return Err(RepoError::EmptyCheckout);
    }

    let mut currency = None;
    let mut lines = Vec::with_capacity(request.lines.len());
    let mut subtotal: MinorUnits = 0;

    for line in &request.lines {
        let priced = price_line(session, line, &mut currency)?;
        subtotal = subtotal
            .checked_add(priced.line_total_minor)
            .ok_or_else(|| RepoError::AmountOverflow {
                variant_id: line.variant_id.clone(),
            })?;
        lines.push(priced);
    }

    let sale = Sale {
        id: ids.next_sale_id(),
        checkout_id: request.checkout_id.clone(),
        created_at: clock.now_rfc3339(),
        currency: currency.unwrap_or_default(),
        subtotal_minor: subtotal,
        total_minor: subtotal,
        lines,
    };

    session.execute(
        "INSERT INTO sales (id, checkout_id, created_at, currency, subtotal_minor,
                            total_minor, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'completed')",
        &[
            Param::Text(&sale.id),
            Param::Text(&sale.checkout_id),
            Param::Text(&sale.created_at),
            Param::Text(&sale.currency),
            Param::Integer(sale.subtotal_minor),
            Param::Integer(sale.total_minor),
        ],
    )?;

    for line in &sale.lines {
        session.execute(
            "INSERT INTO sale_items (sale_id, variant_id, sku, product_name,
                                     unit_price_minor, currency, quantity,
                                     line_total_minor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            &[
                Param::Text(&sale.id),
                Param::Text(&line.variant_id),
                Param::Text(&line.sku),
                Param::Text(&line.product_name),
                Param::Integer(line.unit_price_minor),
                Param::Text(&line.currency),
                Param::Integer(i64::from(line.quantity)),
                Param::Integer(line.line_total_minor),
            ],
        )?;

        // Guarded decrement: `changed == 0` means stock moved under us or was
        // never sufficient, and the whole transaction rolls back.
        let changed = session.execute(
            "UPDATE variants SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1",
            &[
                Param::Integer(i64::from(line.quantity)),
                Param::Text(&line.variant_id),
            ],
        )?;

        if changed == 0 {
            return Err(RepoError::InsufficientStock {
                variant_id: line.variant_id.clone(),
                available: 0,
                requested: line.quantity,
            });
        }
    }

    Ok(CheckoutOutcome::Completed(sale))
}

impl Repository for SqliteRepository {
    fn add_product(&mut self, product: &Product) -> Result<(), RepoError> {
        insert_product(&self.db, product)
    }

    fn add_variant(&mut self, variant: &Variant) -> Result<(), RepoError> {
        if !product_exists(&self.db, &variant.product_id)? {
            return Err(RepoError::UnknownProduct(variant.product_id.clone()));
        }
        insert_variant(&self.db, variant)
    }

    fn load_catalog(&self) -> Result<Catalog, RepoError> {
        load_catalog_from(&self.db)
    }

    fn seed_catalog_if_empty(&mut self, seed: &Catalog) -> Result<SeedOutcome, RepoError> {
        self.db.transaction(|session| seed_in(session, seed))
    }

    fn stock_of(&self, variant_id: &str) -> Result<i64, RepoError> {
        self.db
            .query_row(
                "SELECT stock FROM variants WHERE id = ?1",
                &[Param::Text(variant_id)],
            )?
            .map(|row| row.integer(0))
            .unwrap_or_else(|| Err(RepoError::UnknownVariant(variant_id.to_owned())))
    }

    fn set_price(&mut self, variant_id: &str, price_minor: MinorUnits) -> Result<(), RepoError> {
        let changed = self.db.execute(
            "UPDATE variants SET price_minor = ?1 WHERE id = ?2",
            &[Param::Integer(price_minor), Param::Text(variant_id)],
        )?;

        if changed == 0 {
            return Err(RepoError::UnknownVariant(variant_id.to_owned()));
        }
        Ok(())
    }

    fn checkout(&mut self, request: &CheckoutRequest) -> Result<CheckoutOutcome, RepoError> {
        let Self { db, clock, ids } = self;
        db.transaction(|session| checkout_in(session, request, clock.as_ref(), ids.as_ref()))
    }

    fn sale_by_checkout(&self, checkout_id: &str) -> Result<Option<Sale>, RepoError> {
        load_sale_by_checkout(&self.db, checkout_id)
    }

    fn sale_count(&self) -> Result<i64, RepoError> {
        let row = self
            .db
            .query_row("SELECT COUNT(*) FROM sales", &[])?
            .ok_or_else(|| RepoError::Storage("COUNT returned no row".to_owned()))?;
        row.integer(0)
    }
}
