//! SQLite backend, the candidate engine for the product.
//!
//! # How each `SPEC.md` guarantee is enforced
//!
//! | Guarantee | Mechanism |
//! |---|---|
//! | No partial sale, no wrong stock | One `BEGIN IMMEDIATE` transaction covers the sale row, the line rows, and the stock decrements. Any `?` early-returns and the `Transaction` drop rolls back. |
//! | Retry must not double-record | `sales.checkout_id` is `UNIQUE`; the retry path reads the existing sale inside the same transaction and returns it unchanged. |
//! | Sale keeps its prices | `sale_items` copies `unit_price_minor`, `sku`, and `product_name`. Reading a sale never joins to `variants`. |
//! | Stock can never go negative | `CHECK (stock >= 0)` in the schema plus a guarded `UPDATE ... WHERE stock >= ?`. |
//!
//! The guarded update is deliberately belt-and-braces: even if the pre-check
//! were removed, a concurrent decrement could not drive stock below zero
//! without the statement failing.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::Repository;
use crate::model::{
    CheckoutLine, CheckoutOutcome, CheckoutRequest, Clock, IdSource, MinorUnits, Product,
    RepoError, Sale, SaleLine, Variant,
};

const SCHEMA: &str = include_str!("../migrations/0001_init.sql");

/// Schema version stored in `PRAGMA user_version`.
///
/// A real implementation keeps one `migrations/000N_*.sql` file per version and
/// applies the missing ones in order. The spike only needs the first step, but
/// the mechanism — and the concurrency-safe re-read below — is the part worth
/// proving.
const SCHEMA_VERSION: i64 = 1;

/// SQLite-backed [`Repository`].
pub struct SqliteRepo {
    connection: Connection,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
}

impl SqliteRepo {
    /// Opens an in-memory database and applies the schema.
    pub fn open_in_memory(
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Result<Self, RepoError> {
        Self::from_connection(Connection::open_in_memory()?, clock, ids)
    }

    /// Opens (or creates) a database file and applies the schema.
    pub fn open(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Result<Self, RepoError> {
        Self::from_connection(Connection::open(path)?, clock, ids)
    }

    fn from_connection(
        mut connection: Connection,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Result<Self, RepoError> {
        // Durability matters more than raw throughput for a checkout: losing a
        // committed sale is worse than a slower commit. `execute_batch` is used
        // because `PRAGMA journal_mode` returns a row, which `execute` rejects.
        //
        // `busy_timeout` must be set per connection. Without it, a second
        // checkout racing the first fails with `database is locked` instead of
        // waiting for the write lock and then discovering the replay.
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        Self::migrate(&mut connection)?;
        Ok(Self {
            connection,
            clock,
            ids,
        })
    }

    /// Applies any missing migration.
    ///
    /// The version is read *inside* an `IMMEDIATE` transaction. Reading it
    /// first and then migrating is a race: two processes opening a fresh
    /// database would both see version 0 and both try to create the tables.
    fn migrate(connection: &mut Connection) -> Result<(), RepoError> {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            transaction.execute_batch(SCHEMA)?;
        }

        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
        Ok(())
    }

    fn load_sale_by_checkout(
        connection: &Connection,
        checkout_id: &str,
    ) -> Result<Option<Sale>, RepoError> {
        let sale_id: Option<String> = connection
            .query_row(
                "SELECT id FROM sales WHERE checkout_id = ?1",
                params![checkout_id],
                |row| row.get(0),
            )
            .optional()?;

        match sale_id {
            Some(sale_id) => Self::load_sale(connection, &sale_id).map(Some),
            None => Ok(None),
        }
    }

    fn load_sale(connection: &Connection, sale_id: &str) -> Result<Sale, RepoError> {
        let (id, checkout_id, created_at, currency, subtotal, total) = connection.query_row(
            "SELECT id, checkout_id, created_at, currency, subtotal_minor, total_minor
             FROM sales WHERE id = ?1",
            params![sale_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;

        let mut statement = connection.prepare(
            "SELECT variant_id, sku, product_name, unit_price_minor, currency, quantity,
                    line_total_minor
             FROM sale_items WHERE sale_id = ?1 ORDER BY id",
        )?;
        let lines = statement
            .query_map(params![sale_id], |row| {
                Ok(SaleLine {
                    variant_id: row.get(0)?,
                    sku: row.get(1)?,
                    product_name: row.get(2)?,
                    unit_price_minor: row.get(3)?,
                    currency: row.get(4)?,
                    quantity: row.get::<_, i64>(5)?.try_into().unwrap_or(0),
                    line_total_minor: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(Sale {
            id,
            checkout_id,
            created_at,
            currency,
            subtotal_minor: subtotal,
            total_minor: total,
            lines,
        })
    }

    /// Validates one line and returns the row to insert, without writing.
    fn price_line(
        connection: &Connection,
        line: &CheckoutLine,
        currency: &mut Option<String>,
    ) -> Result<SaleLine, RepoError> {
        if line.quantity == 0 {
            return Err(RepoError::NonPositiveQuantity {
                variant_id: line.variant_id.clone(),
            });
        }

        let (sku, price_minor, line_currency, stock, product_name): (
            String,
            MinorUnits,
            String,
            i64,
            String,
        ) = connection
            .query_row(
                "SELECT v.sku, v.price_minor, v.currency, v.stock, p.name
                 FROM variants v
                 JOIN products p ON p.id = v.product_id
                 WHERE v.id = ?1",
                params![line.variant_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| RepoError::UnknownVariant(line.variant_id.clone()))?;

        if i64::from(line.quantity) > stock {
            return Err(RepoError::InsufficientStock {
                variant_id: line.variant_id.clone(),
                available: stock,
                requested: line.quantity,
            });
        }

        match currency {
            None => *currency = Some(line_currency.clone()),
            Some(expected) if expected != &line_currency => {
                return Err(RepoError::Storage(format!(
                    "variant `{}` uses currency `{line_currency}` but the sale already uses `{expected}`",
                    line.variant_id
                )));
            }
            Some(_) => {}
        }

        Ok(SaleLine {
            variant_id: line.variant_id.clone(),
            sku,
            product_name,
            unit_price_minor: price_minor,
            currency: line_currency,
            quantity: line.quantity,
            line_total_minor: price_minor * i64::from(line.quantity),
        })
    }
}

impl Repository for SqliteRepo {
    fn add_product(&mut self, product: &Product) -> Result<(), RepoError> {
        self.connection.execute(
            "INSERT INTO products (id, name, category, active) VALUES (?1, ?2, ?3, 1)",
            params![product.id, product.name, product.category],
        )?;
        Ok(())
    }

    fn add_variant(&mut self, variant: &Variant) -> Result<(), RepoError> {
        let exists: Option<i64> = self
            .connection
            .query_row(
                "SELECT 1 FROM products WHERE id = ?1",
                params![variant.product_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(RepoError::UnknownProduct(variant.product_id.clone()));
        }

        self.connection.execute(
            "INSERT INTO variants (id, product_id, sku, price_minor, currency, stock)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                variant.id,
                variant.product_id,
                variant.sku,
                variant.price_minor,
                variant.currency,
                variant.stock
            ],
        )?;
        Ok(())
    }

    fn stock_of(&self, variant_id: &str) -> Result<i64, RepoError> {
        self.connection
            .query_row(
                "SELECT stock FROM variants WHERE id = ?1",
                params![variant_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| RepoError::UnknownVariant(variant_id.to_owned()))
    }

    fn set_price(&mut self, variant_id: &str, price_minor: MinorUnits) -> Result<(), RepoError> {
        let changed = self.connection.execute(
            "UPDATE variants SET price_minor = ?1 WHERE id = ?2",
            params![price_minor, variant_id],
        )?;
        if changed == 0 {
            return Err(RepoError::UnknownVariant(variant_id.to_owned()));
        }
        Ok(())
    }

    fn checkout(&mut self, request: &CheckoutRequest) -> Result<CheckoutOutcome, RepoError> {
        // `Immediate` takes the write lock up front. Without it, two concurrent
        // checkouts could both read the same stock and then serialise their
        // writes, and the second would fail at commit time instead of here.
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Retry path: the same checkout key returns the recorded sale and
        // changes nothing. This is checked inside the transaction so a
        // concurrent duplicate cannot slip between the read and the write.
        if let Some(existing) = Self::load_sale_by_checkout(&transaction, &request.checkout_id)? {
            transaction.commit()?;
            return Ok(CheckoutOutcome::Replayed(existing));
        }

        let mut currency = None;
        let mut lines = Vec::with_capacity(request.lines.len());
        for line in &request.lines {
            lines.push(Self::price_line(&transaction, line, &mut currency)?);
        }

        let subtotal: MinorUnits = lines.iter().map(|line| line.line_total_minor).sum();
        let sale = Sale {
            id: self.ids.next_sale_id(),
            checkout_id: request.checkout_id.clone(),
            created_at: self.clock.now_rfc3339(),
            currency: currency.unwrap_or_default(),
            subtotal_minor: subtotal,
            total_minor: subtotal,
            lines,
        };

        transaction.execute(
            "INSERT INTO sales (id, checkout_id, created_at, currency, subtotal_minor,
                                total_minor, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'completed')",
            params![
                sale.id,
                sale.checkout_id,
                sale.created_at,
                sale.currency,
                sale.subtotal_minor,
                sale.total_minor
            ],
        )?;

        for line in &sale.lines {
            transaction.execute(
                "INSERT INTO sale_items (sale_id, variant_id, sku, product_name,
                                         unit_price_minor, currency, quantity,
                                         line_total_minor)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    sale.id,
                    line.variant_id,
                    line.sku,
                    line.product_name,
                    line.unit_price_minor,
                    line.currency,
                    i64::from(line.quantity),
                    line.line_total_minor
                ],
            )?;

            // Guarded decrement: `changed == 0` means the stock moved under us
            // or was never sufficient, and the whole transaction rolls back.
            let changed = transaction.execute(
                "UPDATE variants SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1",
                params![i64::from(line.quantity), line.variant_id],
            )?;
            if changed == 0 {
                return Err(RepoError::InsufficientStock {
                    variant_id: line.variant_id.clone(),
                    available: 0,
                    requested: line.quantity,
                });
            }
        }

        transaction.commit()?;
        Ok(CheckoutOutcome::Completed(sale))
    }

    fn sale_by_checkout(&self, checkout_id: &str) -> Result<Option<Sale>, RepoError> {
        Self::load_sale_by_checkout(&self.connection, checkout_id)
    }

    fn sale_count(&self) -> Result<i64, RepoError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM sales", [], |row| row.get(0))?)
    }
}
