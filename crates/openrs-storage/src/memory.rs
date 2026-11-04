//! In-memory reference implementation.
//!
//! This is the semantic reference: whatever the SQLite backend does, it must
//! match this behavior. Atomicity is achieved by building the next state in a
//! copy and swapping it in only after every validation passes, which is the
//! in-memory analogue of a transaction commit.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::Repository;
use crate::model::{
    Catalog, CheckoutLine, CheckoutOutcome, CheckoutRequest, Clock, IdSource, MinorUnits, Product,
    RepoError, Sale, SaleLine, SeedOutcome, Variant,
};

/// In-memory [`Repository`] used as the behavioral reference and in tests.
pub struct MemoryRepository {
    state: State,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
}

#[derive(Debug, Clone, Default)]
struct State {
    products: BTreeMap<String, Product>,
    variants: BTreeMap<String, Variant>,
    sales: Vec<Sale>,
}

impl MemoryRepository {
    /// Creates an empty repository with the given clock and id source.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, ids: Arc<dyn IdSource>) -> Self {
        Self {
            state: State::default(),
            clock,
            ids,
        }
    }

    /// Applies a checkout to a copy of the state and commits it on success.
    ///
    /// This mirrors `BEGIN IMMEDIATE ... COMMIT` in the SQLite backend: no
    /// caller can observe a partially applied checkout.
    fn apply_checkout(&self, request: &CheckoutRequest) -> Result<(State, Sale), RepoError> {
        if let Some(existing) = self.find_sale(&request.checkout_id) {
            return Ok((self.state.clone(), existing));
        }

        if request.lines.is_empty() {
            return Err(RepoError::EmptyCheckout);
        }

        let mut next = self.state.clone();
        let mut lines = Vec::with_capacity(request.lines.len());
        let mut currency: Option<String> = None;
        let mut subtotal: MinorUnits = 0;

        for CheckoutLine {
            variant_id,
            quantity,
        } in &request.lines
        {
            if *quantity == 0 {
                return Err(RepoError::NonPositiveQuantity {
                    variant_id: variant_id.clone(),
                });
            }

            let variant = next
                .variants
                .get_mut(variant_id)
                .ok_or_else(|| RepoError::UnknownVariant(variant_id.clone()))?;

            if i64::from(*quantity) > variant.stock {
                return Err(RepoError::InsufficientStock {
                    variant_id: variant_id.clone(),
                    available: variant.stock,
                    requested: *quantity,
                });
            }

            if let Some(expected) = &currency {
                if expected != &variant.currency {
                    return Err(RepoError::CurrencyMismatch {
                        variant_id: variant_id.clone(),
                        expected: expected.clone(),
                        found: variant.currency.clone(),
                    });
                }
            } else {
                currency = Some(variant.currency.clone());
            }

            let product_name = next
                .products
                .get(&variant.product_id)
                .map(|product| product.name.clone())
                .ok_or_else(|| RepoError::UnknownProduct(variant.product_id.clone()))?;

            let line_total = variant
                .price_minor
                .checked_mul(i64::from(*quantity))
                .ok_or_else(|| RepoError::AmountOverflow {
                    variant_id: variant_id.clone(),
                })?;
            subtotal =
                subtotal
                    .checked_add(line_total)
                    .ok_or_else(|| RepoError::AmountOverflow {
                        variant_id: variant_id.clone(),
                    })?;

            lines.push(SaleLine {
                variant_id: variant_id.clone(),
                sku: variant.sku.clone(),
                product_name,
                unit_price_minor: variant.price_minor,
                currency: variant.currency.clone(),
                quantity: *quantity,
                line_total_minor: line_total,
            });

            variant.stock -= i64::from(*quantity);
        }

        let sale = Sale {
            id: self.ids.next_sale_id(),
            checkout_id: request.checkout_id.clone(),
            created_at: self.clock.now_rfc3339(),
            currency: currency.unwrap_or_default(),
            subtotal_minor: subtotal,
            total_minor: subtotal,
            lines,
        };

        next.sales.push(sale.clone());
        Ok((next, sale))
    }

    fn find_sale(&self, checkout_id: &str) -> Option<Sale> {
        self.state
            .sales
            .iter()
            .find(|sale| sale.checkout_id == checkout_id)
            .cloned()
    }
}

impl Repository for MemoryRepository {
    fn add_product(&mut self, product: &Product) -> Result<(), RepoError> {
        self.state
            .products
            .insert(product.id.clone(), product.clone());
        Ok(())
    }

    fn add_variant(&mut self, variant: &Variant) -> Result<(), RepoError> {
        if !self.state.products.contains_key(&variant.product_id) {
            return Err(RepoError::UnknownProduct(variant.product_id.clone()));
        }
        self.state
            .variants
            .insert(variant.id.clone(), variant.clone());
        Ok(())
    }

    fn stock_of(&self, variant_id: &str) -> Result<i64, RepoError> {
        self.state
            .variants
            .get(variant_id)
            .map(|variant| variant.stock)
            .ok_or_else(|| RepoError::UnknownVariant(variant_id.to_owned()))
    }

    fn set_price(&mut self, variant_id: &str, price_minor: MinorUnits) -> Result<(), RepoError> {
        let variant = self
            .state
            .variants
            .get_mut(variant_id)
            .ok_or_else(|| RepoError::UnknownVariant(variant_id.to_owned()))?;
        variant.price_minor = price_minor;
        Ok(())
    }

    fn checkout(&mut self, request: &CheckoutRequest) -> Result<CheckoutOutcome, RepoError> {
        let replayed = self.find_sale(&request.checkout_id).is_some();
        let (next, sale) = self.apply_checkout(request)?;
        self.state = next;
        Ok(if replayed {
            CheckoutOutcome::Replayed(sale)
        } else {
            CheckoutOutcome::Completed(sale)
        })
    }

    fn load_catalog(&self) -> Result<Catalog, RepoError> {
        Ok(Catalog::new(
            self.state.products.values().cloned().collect(),
            self.state.variants.values().cloned().collect(),
        ))
    }

    fn seed_catalog_if_empty(&mut self, seed: &Catalog) -> Result<SeedOutcome, RepoError> {
        if !self.state.products.is_empty() {
            return Ok(SeedOutcome::AlreadyPopulated);
        }

        // Build the next state in a copy so a seed catalog that references a
        // missing product leaves the repository empty, matching the SQLite
        // backend, where the transaction rolls back.
        let mut next = self.state.clone();

        for product in seed.products() {
            next.products.insert(product.id.clone(), product.clone());
        }

        for variant in seed.variants() {
            if !next.products.contains_key(&variant.product_id) {
                return Err(RepoError::UnknownProduct(variant.product_id.clone()));
            }
            next.variants.insert(variant.id.clone(), variant.clone());
        }

        self.state = next;
        Ok(SeedOutcome::Seeded)
    }

    fn sale_by_checkout(&self, checkout_id: &str) -> Result<Option<Sale>, RepoError> {
        Ok(self.find_sale(checkout_id))
    }

    fn sale_count(&self) -> Result<i64, RepoError> {
        i64::try_from(self.state.sales.len()).map_err(|error| RepoError::Storage(error.to_string()))
    }
}
