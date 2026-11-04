//! Checks that only a real, file-backed database can exercise.
//!
//! The in-memory backend serialises everything behind `&mut self`, so it cannot
//! show whether data survives a restart or whether two connections racing on the
//! same checkout key are safe. These tests use a database file and independent
//! connections, exactly as two desktop windows would.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use openrs_storage::{
    Catalog, CheckoutLine, CheckoutOutcome, CheckoutRequest, Clock, IdSource, Product, Repository,
    SeedOutcome, SqliteRepository, Variant,
};

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-11-02T00:00:00Z".to_owned()
    }
}

struct SeqIds(AtomicU64);

impl IdSource for SeqIds {
    fn next_sale_id(&self) -> String {
        format!("sale-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// Unique database path per test so parallel test threads never share a file.
fn database_path(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("openrs-storage-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.join(format!("{label}.sqlite"))
}

fn seed(path: &PathBuf, stock: i64) {
    let mut repository = SqliteRepository::open(
        path,
        Arc::new(FixedClock),
        Arc::new(SeqIds(AtomicU64::new(0))),
    )
    .expect("open");

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
            stock,
        })
        .expect("add variant");
}

fn request(quantity: u32) -> CheckoutRequest {
    CheckoutRequest {
        checkout_id: "checkout-shared".to_owned(),
        lines: vec![CheckoutLine {
            variant_id: "v1".to_owned(),
            quantity,
        }],
    }
}

#[test]
fn sale_and_stock_survive_a_reopen() {
    let path = database_path("reopen");
    seed(&path, 10);

    {
        let mut repository = SqliteRepository::open(
            &path,
            Arc::new(FixedClock),
            Arc::new(SeqIds(AtomicU64::new(0))),
        )
        .expect("open");
        repository.checkout(&request(4)).expect("checkout");
    }

    // A second repository is a second connection to the same file, as after an
    // application restart.
    let repository = SqliteRepository::open(
        &path,
        Arc::new(FixedClock),
        Arc::new(SeqIds(AtomicU64::new(0))),
    )
    .expect("reopen");

    assert_eq!(repository.stock_of("v1").expect("stock"), 6);
    assert_eq!(repository.sale_count().expect("count"), 1);
    let sale = repository
        .sale_by_checkout("checkout-shared")
        .expect("lookup")
        .expect("sale exists");
    assert_eq!(sale.subtotal_minor, 1_999 * 4);
    assert_eq!(sale.quantity_of("v1"), 4);

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn the_seeded_catalog_survives_a_restart_and_is_not_seeded_again() {
    let path = database_path("catalog");
    let open = || {
        SqliteRepository::open(
            &path,
            Arc::new(FixedClock),
            Arc::new(SeqIds(AtomicU64::new(0))),
        )
        .expect("open")
    };

    {
        let mut repository = open();
        assert_eq!(
            repository
                .seed_catalog_if_empty(&Catalog::sample())
                .expect("seed"),
            SeedOutcome::Seeded
        );
    }

    // A second connection is what the next application start sees.
    let mut reopened = open();
    assert_eq!(
        reopened.load_catalog().expect("load"),
        Catalog::sample(),
        "the catalog must survive a restart"
    );
    assert_eq!(
        reopened
            .seed_catalog_if_empty(&Catalog::sample())
            .expect("seed again"),
        SeedOutcome::AlreadyPopulated,
        "a populated database must not be seeded again"
    );
    assert_eq!(
        reopened.load_catalog().expect("load").products().len(),
        4,
        "re-seeding must not duplicate products"
    );

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn two_connections_racing_on_the_same_checkout_key_record_one_sale() {
    let path = database_path("race");
    seed(&path, 10);

    // A single shared id source, because two independent counters would both
    // start at `sale-1` and collide on the primary key. That collision is the
    // reason the product uses UUIDv7 sale ids.
    let ids = Arc::new(SeqIds(AtomicU64::new(0)));

    let run = |ids: Arc<SeqIds>| {
        let path = path.clone();
        thread::spawn(move || {
            let mut repository =
                SqliteRepository::open(&path, Arc::new(FixedClock), ids).expect("open in thread");
            repository.checkout(&request(4))
        })
    };

    let first = run(Arc::clone(&ids));
    let second = run(Arc::clone(&ids));

    let results = [first.join().expect("join"), second.join().expect("join")];

    let completed = results
        .iter()
        .filter(|result| matches!(result, Ok(CheckoutOutcome::Completed(_))))
        .count();
    let replayed = results
        .iter()
        .filter(|result| matches!(result, Ok(CheckoutOutcome::Replayed(_))))
        .count();

    assert_eq!(completed, 1, "exactly one call may create the sale");
    assert_eq!(replayed, 1, "the other call must be a replay");
    assert!(
        results.iter().all(Result::is_ok),
        "no call may fail: {results:?}"
    );

    let repository = SqliteRepository::open(
        &path,
        Arc::new(FixedClock),
        Arc::new(SeqIds(AtomicU64::new(0))),
    )
    .expect("reopen");
    assert_eq!(repository.stock_of("v1").expect("stock"), 6);
    assert_eq!(repository.sale_count().expect("count"), 1);

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn competing_checkouts_cannot_oversell() {
    let path = database_path("oversell");
    seed(&path, 5);

    let ids = Arc::new(SeqIds(AtomicU64::new(0)));
    let run = |ids: Arc<SeqIds>, checkout_id: &'static str| {
        let path = path.clone();
        thread::spawn(move || {
            let mut repository =
                SqliteRepository::open(&path, Arc::new(FixedClock), ids).expect("open in thread");
            repository.checkout(&CheckoutRequest {
                checkout_id: checkout_id.to_owned(),
                lines: vec![CheckoutLine {
                    variant_id: "v1".to_owned(),
                    quantity: 4,
                }],
            })
        })
    };

    // Two different checkouts, each wanting 4 of 5 units. Exactly one may win.
    let a = run(Arc::clone(&ids), "checkout-a");
    let b = run(Arc::clone(&ids), "checkout-b");
    let results = [a.join().expect("join"), b.join().expect("join")];

    let completed = results
        .iter()
        .filter(|result| matches!(result, Ok(CheckoutOutcome::Completed(_))))
        .count();

    assert_eq!(completed, 1, "only one checkout may succeed: {results:?}");

    let repository = SqliteRepository::open(
        &path,
        Arc::new(FixedClock),
        Arc::new(SeqIds(AtomicU64::new(0))),
    )
    .expect("reopen");
    assert_eq!(
        repository.stock_of("v1").expect("stock"),
        1,
        "stock never negative"
    );

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}
