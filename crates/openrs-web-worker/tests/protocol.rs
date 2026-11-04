//! The worker protocol, exercised against a real repository.
//!
//! This runs twice, from the same bodies:
//!
//! - natively with `cargo test -p openrs-web-worker`;
//! - in headless Chromium with `wasm-pack test --headless --chrome
//!   crates/openrs-web-worker`, which is the closest the protocol gets to the
//!   browser without starting the actual worker.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use openrs_core::{
    CheckoutLine, CheckoutReply, CheckoutRequest, checkout_request_message, decode_checkout_reply,
};
use openrs_storage::{Catalog, Clock, IdSource, Product, Repository, SqliteRepository, Variant};
use openrs_web_worker::handle_message;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

/// Declares one test that also runs in the browser.
#[cfg(not(target_arch = "wasm32"))]
macro_rules! worker_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() $body
    };
}

#[cfg(target_arch = "wasm32")]
macro_rules! worker_test {
    ($name:ident, $body:block) => {
        #[wasm_bindgen_test::wasm_bindgen_test]
        fn $name() $body
    };
}

const PRICE: i64 = 1_999;
const STOCK: i64 = 5;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-11-04T00:00:00Z".to_owned()
    }
}

struct SeqIds(AtomicU64);

impl IdSource for SeqIds {
    fn next_sale_id(&self) -> String {
        format!("sale-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// A repository holding one product with one variant of [`STOCK`] units.
fn seeded() -> SqliteRepository {
    let mut repository = SqliteRepository::open_in_memory(
        Arc::new(FixedClock),
        Arc::new(SeqIds(AtomicU64::new(0))),
    )
    .expect("open sqlite");

    let catalog = Catalog::new(
        vec![Product {
            id: "p-1".to_owned(),
            name: "Test Shirt".to_owned(),
            category: "Apparel".to_owned(),
        }],
        vec![Variant {
            id: "v-1".to_owned(),
            product_id: "p-1".to_owned(),
            sku: "SKU-1".to_owned(),
            price_minor: PRICE,
            currency: "USD".to_owned(),
            stock: STOCK,
        }],
    );

    repository.seed_catalog_if_empty(&catalog).expect("seed");
    repository
}

fn request(checkout_id: &str, quantity: u32) -> CheckoutRequest {
    CheckoutRequest {
        checkout_id: checkout_id.to_owned(),
        lines: vec![CheckoutLine {
            variant_id: "v-1".to_owned(),
            quantity,
        }],
    }
}

fn stock_in(catalog: &Catalog, variant_id: &str) -> i64 {
    catalog
        .variants()
        .iter()
        .find(|variant| variant.id == variant_id)
        .map_or_else(
            || panic!("variant `{variant_id}` missing from the reply catalog"),
            |variant| variant.stock,
        )
}

worker_test!(a_checkout_request_records_the_sale_and_returns_the_catalog, {
    let mut repository = seeded();
    let request = request("checkout-1", 2);

    let reply = handle_message(&mut repository, &checkout_request_message(&request));

    match decode_checkout_reply(&reply).expect("decode the reply") {
        CheckoutReply::Recorded { receipt, catalog } => {
            assert_eq!(receipt.checkout_id, "checkout-1");
            assert_eq!(receipt.total_minor, 2 * PRICE);
            assert_eq!(receipt.currency, "USD");
            assert_eq!(
                stock_in(&catalog, "v-1"),
                STOCK - 2,
                "the reply catalog must show the deducted stock"
            );
        }
        other => panic!("expected a recorded sale, got {other:?}"),
    }

    assert_eq!(repository.sale_count().expect("count"), 1);
    assert_eq!(repository.stock_of("v-1").expect("stock"), STOCK - 2);
});

worker_test!(a_repeated_request_returns_the_same_sale, {
    let mut repository = seeded();
    let request = request("checkout-1", 2);
    let message = checkout_request_message(&request);

    let first = handle_message(&mut repository, &message);
    let second = handle_message(&mut repository, &message);

    let sale_id = |reply: &str| match decode_checkout_reply(reply).expect("decode") {
        CheckoutReply::Recorded { receipt, .. } => receipt.sale_id,
        other => panic!("expected a recorded sale, got {other:?}"),
    };

    assert_eq!(
        sale_id(&first),
        sale_id(&second),
        "a replay must return the sale that was already recorded"
    );
    assert_eq!(repository.sale_count().expect("count"), 1);
    assert_eq!(
        repository.stock_of("v-1").expect("stock"),
        STOCK - 2,
        "a replay must not deduct stock twice"
    );
});

worker_test!(an_oversold_request_replies_with_its_key_and_writes_nothing, {
    let mut repository = seeded();
    let request = request("checkout-2", u32::try_from(STOCK + 1).expect("quantity"));

    let reply = handle_message(&mut repository, &checkout_request_message(&request));

    match decode_checkout_reply(&reply).expect("decode") {
        CheckoutReply::Failed {
            checkout_id,
            reason,
        } => {
            assert_eq!(checkout_id, "checkout-2", "the UI needs its key back");
            assert!(
                reason.contains("insufficient stock"),
                "unexpected reason `{reason}`"
            );
        }
        other => panic!("expected a rejection, got {other:?}"),
    }

    assert_eq!(repository.sale_count().expect("count"), 0);
    assert_eq!(repository.stock_of("v-1").expect("stock"), STOCK);
});

worker_test!(an_unreadable_message_is_reported_without_a_key, {
    let mut repository = seeded();

    let reply = handle_message(&mut repository, "not a checkout");

    match decode_checkout_reply(&reply).expect("decode") {
        CheckoutReply::Failed {
            checkout_id,
            reason,
        } => {
            assert!(checkout_id.is_empty(), "there is no attempt to attribute");
            assert!(reason.contains("could not read"), "unexpected `{reason}`");
        }
        other => panic!("expected a rejection, got {other:?}"),
    }

    assert_eq!(repository.sale_count().expect("count"), 0);
});

worker_test!(a_rejected_request_keeps_its_key_free_for_the_real_attempt, {
    let mut repository = seeded();

    // Same key, first too large, then valid: the rejected attempt must not have
    // consumed the key, or the corrected retry would look like a replay.
    let rejected = handle_message(
        &mut repository,
        &checkout_request_message(&request("checkout-3", 99)),
    );
    assert!(matches!(
        decode_checkout_reply(&rejected).expect("decode"),
        CheckoutReply::Failed { .. }
    ));

    let accepted = handle_message(
        &mut repository,
        &checkout_request_message(&request("checkout-3", 1)),
    );
    assert!(matches!(
        decode_checkout_reply(&accepted).expect("decode"),
        CheckoutReply::Recorded { .. }
    ));

    assert_eq!(repository.sale_count().expect("count"), 1);
});
