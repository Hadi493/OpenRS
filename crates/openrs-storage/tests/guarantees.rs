//! The `SPEC.md` sale guarantees, executed against **both** backends.
//!
//! This file compiles to two different test binaries:
//!
//! - natively it is a normal `cargo test` target;
//! - on `wasm32-unknown-unknown` it runs the same bodies in headless Chromium
//!   through `wasm-bindgen-test`.
//!
//! Every assertion is tagged with the backend name, so a divergence between the
//! in-memory reference and SQLite is reported directly instead of looking like a
//! generic failure.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use openrs_storage::{
    Catalog, CheckoutLine, CheckoutOutcome, CheckoutRequest, Clock, IdSource, MemoryRepository,
    Product, RepoError, Repository, Sale, SeedOutcome, SqliteRepository, Variant,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

/// Declares one test that also runs in the browser on WASM.
///
/// The body is shared; only the attribute differs per target.
#[cfg(not(target_arch = "wasm32"))]
macro_rules! guarantee_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() $body
    };
}

#[cfg(target_arch = "wasm32")]
macro_rules! guarantee_test {
    ($name:ident, $body:block) => {
        #[wasm_bindgen_test::wasm_bindgen_test]
        fn $name() $body
    };
}

const PRICE: i64 = 1_999;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-11-02T00:00:00Z".to_owned()
    }
}

struct SeqIds(AtomicU64);

impl SeqIds {
    fn new() -> Self {
        Self(AtomicU64::new(0))
    }
}

impl IdSource for SeqIds {
    fn next_sale_id(&self) -> String {
        format!("sale-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// Constructor for one backend, paired with a display name.
type BackendFactory = Box<dyn Fn() -> Box<dyn Repository>>;

fn backends() -> Vec<(&'static str, BackendFactory)> {
    vec![
        (
            "memory",
            Box::new(|| {
                Box::new(MemoryRepository::new(
                    Arc::new(FixedClock),
                    Arc::new(SeqIds::new()),
                )) as Box<dyn Repository>
            }),
        ),
        (
            "sqlite",
            Box::new(|| {
                Box::new(
                    SqliteRepository::open_in_memory(Arc::new(FixedClock), Arc::new(SeqIds::new()))
                        .expect("open sqlite"),
                ) as Box<dyn Repository>
            }),
        ),
    ]
}

/// Runs `test` once per backend, tagging failures with the backend name.
fn for_each_backend(test: &dyn Fn(&str, &mut dyn Repository)) {
    for (name, make) in backends() {
        let mut repository = make();
        test(name, repository.as_mut());
    }
}

/// Seeds one product with one variant: price [`PRICE`], given stock.
fn seed(repository: &mut dyn Repository, variant_id: &str, stock: i64) {
    repository
        .add_product(&Product {
            id: "product-1".to_owned(),
            name: "Test T-Shirt".to_owned(),
            category: "Apparel".to_owned(),
        })
        .expect("add product");

    repository
        .add_variant(&Variant {
            id: variant_id.to_owned(),
            product_id: "product-1".to_owned(),
            sku: format!("SKU-{variant_id}"),
            price_minor: PRICE,
            currency: "USD".to_owned(),
            stock,
        })
        .expect("add variant");
}

fn request(checkout_id: &str, variant_id: &str, quantity: u32) -> CheckoutRequest {
    CheckoutRequest {
        checkout_id: checkout_id.to_owned(),
        lines: vec![CheckoutLine {
            variant_id: variant_id.to_owned(),
            quantity,
        }],
    }
}

fn completed(outcome: CheckoutOutcome) -> Sale {
    match outcome {
        CheckoutOutcome::Completed(sale) => sale,
        CheckoutOutcome::Replayed(sale) => {
            panic!("expected a new sale, got a replay of {}", sale.id)
        }
    }
}

guarantee_test!(a_fresh_database_seeds_the_catalog_exactly_once, {
    for_each_backend(&|name, repository| {
        assert!(
            repository
                .load_catalog()
                .expect("load")
                .products()
                .is_empty(),
            "{name}: a fresh database must be empty"
        );

        let outcome = repository
            .seed_catalog_if_empty(&Catalog::sample())
            .expect("seed");
        assert_eq!(outcome, SeedOutcome::Seeded, "{name}: first seed");

        let outcome = repository
            .seed_catalog_if_empty(&Catalog::sample())
            .expect("seed again");
        assert_eq!(
            outcome,
            SeedOutcome::AlreadyPopulated,
            "{name}: a second seed must not write anything"
        );

        let loaded = repository.load_catalog().expect("load");
        assert_eq!(
            loaded.products().len(),
            4,
            "{name}: seeded products must not be duplicated"
        );
        assert_eq!(
            loaded.variants().len(),
            7,
            "{name}: seeded variants must not be duplicated"
        );
    });
});

guarantee_test!(the_loaded_catalog_matches_the_seed_in_catalog_order, {
    for_each_backend(&|name, repository| {
        let seed = Catalog::sample();
        repository.seed_catalog_if_empty(&seed).expect("seed");

        let loaded = repository.load_catalog().expect("load");

        assert_eq!(loaded, seed, "{name}: round trip must be lossless");
        assert_eq!(
            loaded.find("tee-m", None)[0].variants[0].sku,
            "TEE-M",
            "{name}: loaded rows must answer the same listing query"
        );
    });
});

guarantee_test!(a_seed_with_an_orphan_variant_writes_nothing, {
    for_each_backend(&|name, repository| {
        let broken = Catalog::new(
            vec![Product {
                id: "product-1".to_owned(),
                name: "Test T-Shirt".to_owned(),
                category: "Apparel".to_owned(),
            }],
            vec![Variant {
                id: "v-orphan".to_owned(),
                product_id: "missing-product".to_owned(),
                sku: "SKU-ORPHAN".to_owned(),
                price_minor: 100,
                currency: "USD".to_owned(),
                stock: 1,
            }],
        );

        let error = repository
            .seed_catalog_if_empty(&broken)
            .expect_err("the seed must be rejected");
        assert!(
            matches!(error, RepoError::UnknownProduct(_)),
            "{name}: unexpected error {error:?}"
        );

        let loaded = repository.load_catalog().expect("load");
        assert!(
            loaded.products().is_empty() && loaded.variants().is_empty(),
            "{name}: a failed seed must not leave partial rows"
        );

        // The database is still seedable afterwards, which proves the failed
        // attempt did not half-populate it.
        assert_eq!(
            repository
                .seed_catalog_if_empty(&Catalog::sample())
                .expect("seed"),
            SeedOutcome::Seeded,
            "{name}: a valid seed must still work"
        );
    });
});

guarantee_test!(a_sale_decrements_the_catalog_stock_that_is_loaded, {
    for_each_backend(&|name, repository| {
        repository
            .seed_catalog_if_empty(&Catalog::sample())
            .expect("seed");

        repository
            .checkout(&request("checkout-1", "v-tee-s", 2))
            .expect("checkout");

        let loaded = repository.load_catalog().expect("load");
        let sold = loaded
            .variants()
            .iter()
            .find(|variant| variant.id == "v-tee-s")
            .expect("the sold variant is still listed");

        assert_eq!(sold.stock, 10, "{name}: 12 minus 2");
    });
});

guarantee_test!(checkout_records_the_sale_and_decrements_stock, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 10);

        let sale = completed(
            repository
                .checkout(&request("checkout-1", "v1", 3))
                .expect("checkout"),
        );

        assert_eq!(sale.subtotal_minor, PRICE * 3, "{name}: subtotal");
        assert_eq!(sale.total_minor, sale.subtotal_minor, "{name}: total");
        assert_eq!(sale.lines.len(), 1, "{name}: one line");
        assert_eq!(sale.quantity_of("v1"), 3, "{name}: quantity snapshot");
        assert_eq!(
            repository.stock_of("v1").expect("stock"),
            7,
            "{name}: stock after sale"
        );
        assert_eq!(
            repository.sale_count().expect("count"),
            1,
            "{name}: one sale"
        );
    });
});

guarantee_test!(insufficient_stock_leaves_no_sale_and_no_stock_change, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 2);

        let error = repository
            .checkout(&request("checkout-1", "v1", 3))
            .expect_err("checkout must be rejected");

        assert!(
            matches!(
                error,
                RepoError::InsufficientStock {
                    requested: 3,
                    available: 2,
                    ..
                }
            ),
            "{name}: unexpected error {error:?}"
        );
        assert_eq!(
            repository.stock_of("v1").expect("stock"),
            2,
            "{name}: stock"
        );
        assert_eq!(
            repository.sale_count().expect("count"),
            0,
            "{name}: no sale"
        );
        assert!(
            repository
                .sale_by_checkout("checkout-1")
                .expect("lookup")
                .is_none(),
            "{name}: no sale row"
        );
    });
});

guarantee_test!(
    a_multi_line_checkout_fails_atomically_when_the_second_line_is_short,
    {
        for_each_backend(&|name, repository| {
            seed(repository, "v1", 10);
            repository
                .add_variant(&Variant {
                    id: "v2".to_owned(),
                    product_id: "product-1".to_owned(),
                    sku: "SKU-v2".to_owned(),
                    price_minor: 500,
                    currency: "USD".to_owned(),
                    stock: 1,
                })
                .expect("add second variant");

            let request = CheckoutRequest {
                checkout_id: "checkout-1".to_owned(),
                lines: vec![
                    CheckoutLine {
                        variant_id: "v1".to_owned(),
                        quantity: 2,
                    },
                    CheckoutLine {
                        variant_id: "v2".to_owned(),
                        quantity: 5,
                    },
                ],
            };

            let error = repository.checkout(&request).expect_err("must reject");
            assert!(
                matches!(error, RepoError::InsufficientStock { .. }),
                "{name}: unexpected error {error:?}"
            );

            // The first line must not have been deducted: no partial sale.
            assert_eq!(
                repository.stock_of("v1").expect("stock"),
                10,
                "{name}: first line must not be deducted"
            );
            assert_eq!(
                repository.stock_of("v2").expect("stock"),
                1,
                "{name}: second line untouched"
            );
            assert_eq!(
                repository.sale_count().expect("count"),
                0,
                "{name}: no sale"
            );
        });
    }
);

guarantee_test!(repeated_checkout_with_the_same_key_is_a_replay, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 10);
        let request = request("checkout-1", "v1", 4);

        let first = completed(repository.checkout(&request).expect("first checkout"));

        let second = repository.checkout(&request).expect("retry");
        match second {
            CheckoutOutcome::Replayed(replayed) => {
                assert_eq!(replayed.id, first.id, "{name}: same sale id");
                assert_eq!(
                    replayed.created_at, first.created_at,
                    "{name}: timestamp unchanged"
                );
            }
            CheckoutOutcome::Completed(sale) => {
                panic!("{name}: retry created a second sale {}", sale.id);
            }
        }

        assert_eq!(
            repository.stock_of("v1").expect("stock"),
            6,
            "{name}: stock deducted exactly once"
        );
        assert_eq!(
            repository.sale_count().expect("count"),
            1,
            "{name}: exactly one sale"
        );
    });
});

guarantee_test!(
    a_recorded_sale_keeps_its_prices_after_a_catalog_price_change,
    {
        for_each_backend(&|name, repository| {
            seed(repository, "v1", 10);
            let sale = completed(
                repository
                    .checkout(&request("checkout-1", "v1", 2))
                    .expect("checkout"),
            );

            repository.set_price("v1", 999_999).expect("price edit");

            let reloaded = repository
                .sale_by_checkout("checkout-1")
                .expect("lookup")
                .expect("sale exists");

            assert_eq!(reloaded, sale, "{name}: stored sale must be immutable");
            assert_eq!(
                reloaded.lines[0].unit_price_minor, PRICE,
                "{name}: historical unit price"
            );
            assert_eq!(reloaded.total_minor, PRICE * 2, "{name}: historical total");
            assert_eq!(
                reloaded.lines[0].product_name, "Test T-Shirt",
                "{name}: historical product name"
            );
        });
    }
);

guarantee_test!(zero_quantity_is_rejected, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 10);

        let error = repository
            .checkout(&request("checkout-1", "v1", 0))
            .expect_err("must reject");

        assert!(
            matches!(error, RepoError::NonPositiveQuantity { .. }),
            "{name}: unexpected error {error:?}"
        );
        assert_eq!(
            repository.stock_of("v1").expect("stock"),
            10,
            "{name}: stock"
        );
        assert_eq!(
            repository.sale_count().expect("count"),
            0,
            "{name}: no sale"
        );
    });
});

guarantee_test!(unknown_variant_is_rejected, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 10);

        let error = repository
            .checkout(&request("checkout-1", "does-not-exist", 1))
            .expect_err("must reject");

        assert!(
            matches!(error, RepoError::UnknownVariant(_)),
            "{name}: unexpected error {error:?}"
        );
        assert_eq!(
            repository.sale_count().expect("count"),
            0,
            "{name}: no sale"
        );
    });
});

guarantee_test!(totals_are_exact_integer_minor_units, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 10);
        repository
            .add_variant(&Variant {
                id: "v2".to_owned(),
                product_id: "product-1".to_owned(),
                sku: "SKU-v2".to_owned(),
                price_minor: 1,
                currency: "USD".to_owned(),
                stock: 100,
            })
            .expect("add second variant");

        let request = CheckoutRequest {
            checkout_id: "checkout-1".to_owned(),
            lines: vec![
                CheckoutLine {
                    variant_id: "v1".to_owned(),
                    quantity: 3,
                },
                CheckoutLine {
                    variant_id: "v2".to_owned(),
                    quantity: 7,
                },
            ],
        };

        let sale = completed(repository.checkout(&request).expect("checkout"));

        assert_eq!(
            sale.subtotal_minor,
            PRICE * 3 + 7,
            "{name}: exact integer subtotal"
        );
        assert_eq!(
            sale.lines[0].line_total_minor,
            PRICE * 3,
            "{name}: exact line total"
        );
        assert_eq!(sale.currency, "USD", "{name}: currency");
    });
});

guarantee_test!(mixed_currencies_are_rejected, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 10);
        repository
            .add_variant(&Variant {
                id: "v2".to_owned(),
                product_id: "product-1".to_owned(),
                sku: "SKU-v2".to_owned(),
                price_minor: 500,
                currency: "EUR".to_owned(),
                stock: 5,
            })
            .expect("add second variant");

        let request = CheckoutRequest {
            checkout_id: "checkout-1".to_owned(),
            lines: vec![
                CheckoutLine {
                    variant_id: "v1".to_owned(),
                    quantity: 1,
                },
                CheckoutLine {
                    variant_id: "v2".to_owned(),
                    quantity: 1,
                },
            ],
        };

        let error = repository.checkout(&request).expect_err("must reject");
        assert!(
            matches!(error, RepoError::CurrencyMismatch { .. }),
            "{name}: unexpected error {error:?}"
        );
        assert_eq!(
            repository.stock_of("v1").expect("stock"),
            10,
            "{name}: no partial deduction"
        );
        assert_eq!(
            repository.sale_count().expect("count"),
            0,
            "{name}: no sale"
        );
    });
});

guarantee_test!(an_empty_checkout_records_nothing, {
    for_each_backend(&|name, repository| {
        seed(repository, "v1", 5);

        let request = CheckoutRequest {
            checkout_id: "checkout-empty".to_owned(),
            lines: Vec::new(),
        };

        let error = repository.checkout(&request).expect_err("must reject");
        assert!(
            matches!(error, RepoError::EmptyCheckout),
            "{name}: unexpected error {error:?}"
        );
        assert_eq!(
            repository.sale_count().expect("count"),
            0,
            "{name}: no sale"
        );
        assert_eq!(
            repository.stock_of("v1").expect("stock"),
            5,
            "{name}: no deduction"
        );
        assert!(
            repository
                .sale_by_checkout("checkout-empty")
                .expect("lookup")
                .is_none(),
            "{name}: the rejected key must stay free for a later attempt"
        );
    });
});
