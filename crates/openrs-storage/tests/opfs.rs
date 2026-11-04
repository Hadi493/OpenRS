//! Browser persistence tests on the real product storage layer.
//!
//! These run in a dedicated worker because OPFS sync access handles do not exist
//! on the main thread, and the `sahpool` VFS needs cross-origin isolation. They
//! prove that the sale recorded through `SqliteRepository` lives in OPFS and
//! survives a closed connection, and that the persisted idempotency key still
//! replays after reopening.

#![cfg(target_arch = "wasm32")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use openrs_storage::{
    Catalog, CheckoutLine, CheckoutOutcome, CheckoutRequest, Clock, IdSource, Product, Repository,
    SeedOutcome, SqliteRepository, Variant,
};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const DATABASE: &str = "openrs-storage-test.db";

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

async fn open() -> SqliteRepository {
    SqliteRepository::open_opfs(DATABASE, Arc::new(FixedClock), Arc::new(SeqIds::new()))
        .await
        .expect("open the OPFS database")
}

fn request() -> CheckoutRequest {
    CheckoutRequest {
        checkout_id: "checkout-opfs".to_owned(),
        lines: vec![CheckoutLine {
            variant_id: "v1".to_owned(),
            quantity: 3,
        }],
    }
}

#[wasm_bindgen_test]
async fn sale_and_stock_survive_a_reconnect_on_opfs() {
    let mut repository = open().await;

    repository
        .add_product(&Product {
            id: "product-1".to_owned(),
            name: "Test T-Shirt".to_owned(),
            category: "Apparel".to_owned(),
        })
        .expect("add product");
    repository
        .add_variant(&Variant {
            id: "v1".to_owned(),
            product_id: "product-1".to_owned(),
            sku: "SKU-v1".to_owned(),
            price_minor: 1_999,
            currency: "USD".to_owned(),
            stock: 10,
        })
        .expect("add variant");

    let sale = match repository.checkout(&request()).expect("checkout") {
        CheckoutOutcome::Completed(sale) => sale,
        CheckoutOutcome::Replayed(sale) => panic!("unexpected replay of {}", sale.id),
    };
    assert_eq!(repository.stock_of("v1").expect("stock"), 7);

    // Dropping closes the connection, which forces the data out to OPFS. A
    // memory-only database would be empty after this point.
    drop(repository);

    let mut reopened = open().await;
    assert_eq!(
        reopened.stock_of("v1").expect("stock"),
        7,
        "stock must survive on OPFS"
    );
    assert_eq!(reopened.sale_count().expect("count"), 1);

    let stored = reopened
        .sale_by_checkout("checkout-opfs")
        .expect("lookup")
        .expect("sale must survive on OPFS");
    assert_eq!(stored, sale, "the stored sale must be unchanged");

    // The durable idempotency key still works after reopening, so a retry after
    // a crash cannot record a second sale.
    let retry = reopened.checkout(&request()).expect("retry");
    match retry {
        CheckoutOutcome::Replayed(replayed) => assert_eq!(replayed.id, sale.id),
        CheckoutOutcome::Completed(duplicate) => {
            panic!("retry created a second sale {}", duplicate.id)
        }
    }
    assert_eq!(reopened.sale_count().expect("count"), 1);
    assert_eq!(reopened.stock_of("v1").expect("stock"), 7);
}

#[wasm_bindgen_test]
async fn the_catalog_survives_a_reconnect_and_is_seeded_only_once() {
    {
        let mut repository = open().await;
        assert_eq!(
            repository
                .seed_catalog_if_empty(&Catalog::sample())
                .expect("seed"),
            SeedOutcome::Seeded,
            "the test database starts empty"
        );
    }

    // Reopening is what the next page load does: the catalog must come back from
    // OPFS rather than from the seed.
    let mut reopened = open().await;
    assert_eq!(
        reopened.load_catalog().expect("load"),
        Catalog::sample(),
        "the catalog must survive in OPFS"
    );
    assert_eq!(
        reopened
            .seed_catalog_if_empty(&Catalog::sample())
            .expect("seed again"),
        SeedOutcome::AlreadyPopulated,
        "a second load must not seed again"
    );
    assert_eq!(
        reopened.load_catalog().expect("load").products().len(),
        4,
        "re-seeding must not duplicate products"
    );
}

#[wasm_bindgen_test]
async fn opening_the_same_opfs_database_twice_keeps_the_schema() {
    let first = open().await;
    let before = first.sale_count().expect("count");
    drop(first);

    let second = open().await;
    // Migration runs again on open. It must be idempotent, not fail on
    // existing tables, and it must not reset or duplicate data.
    assert_eq!(
        second.sale_count().expect("count"),
        before,
        "reopening must preserve the recorded sales"
    );
}
