//! Cross-target bill contract for `BILL-001`.
//!
//! `SPEC.md` requires the bill subtotal to be calculated from the items and
//! quantities in the bill, and asks for the rule to be verified by shared Rust
//! unit tests **and** desktop/WebAssembly acceptance tests. This file supplies
//! the acceptance half: one assertion body runs as a plain `#[test]` under
//! `cargo test -p openrs-core` and as a `wasm_bindgen_test` in Chromium under
//! `wasm-pack test --headless --chrome crates/openrs-core`.
//!
//! The body deliberately exercises the preview rules a cashier depends on, not
//! only the happy path: exact totals, merged quantities, removal, and the
//! rejections that must leave the bill unchanged.

use openrs_core::{Bill, BillError, Catalog, Product, Variant, format_subtotal};

fn catalog() -> Catalog {
    Catalog::new(
        vec![
            Product {
                id: "p-tee".to_owned(),
                name: "Cotton T-Shirt".to_owned(),
                category: "Apparel".to_owned(),
            },
            Product {
                id: "p-mug".to_owned(),
                name: "Ceramic Mug".to_owned(),
                category: "Homeware".to_owned(),
            },
        ],
        vec![
            Variant {
                id: "v-tee-s".to_owned(),
                product_id: "p-tee".to_owned(),
                sku: "TEE-S".to_owned(),
                price_minor: 1_499,
                currency: "USD".to_owned(),
                stock: 5,
            },
            Variant {
                id: "v-mug".to_owned(),
                product_id: "p-mug".to_owned(),
                sku: "MUG-WHT".to_owned(),
                price_minor: 1_250,
                currency: "USD".to_owned(),
                stock: 2,
            },
            Variant {
                id: "v-tee-l".to_owned(),
                product_id: "p-tee".to_owned(),
                sku: "TEE-L".to_owned(),
                price_minor: 1_599,
                currency: "USD".to_owned(),
                stock: 0,
            },
        ],
    )
}

/// Bill behavior every target must reproduce.
fn assert_bill_contract() {
    let catalog = catalog();

    // BILL-001: the subtotal is the sum of the line totals.
    let mut bill = Bill::new();
    bill.add(&catalog, "v-tee-s", 2).expect("add tee");
    bill.add(&catalog, "v-mug", 1).expect("add mug");

    assert_eq!(bill.len(), 2);
    assert_eq!(bill.lines()[0].line_total_minor, 2_998);
    assert_eq!(bill.lines()[1].line_total_minor, 1_250);
    assert_eq!(bill.subtotal_minor(), 4_248);
    assert_eq!(
        format_subtotal(bill.subtotal_minor(), bill.currency()),
        "42.48 USD"
    );

    // Adding the same variant merges quantities instead of adding a row.
    bill.add(&catalog, "v-tee-s", 1).expect("merge");
    assert_eq!(bill.len(), 2);
    assert_eq!(bill.lines()[0].quantity, 3);
    assert_eq!(bill.subtotal_minor(), 1_499 * 3 + 1_250);

    // Changing a quantity recomputes the preview; zero removes the line.
    bill.set_quantity(&catalog, "v-tee-s", 1).expect("set");
    assert_eq!(bill.subtotal_minor(), 1_499 + 1_250);
    bill.set_quantity(&catalog, "v-mug", 0)
        .expect("remove via zero");
    assert_eq!(bill.len(), 1);
    assert_eq!(bill.subtotal_minor(), 1_499);

    // Invalid changes are rejected and leave the bill untouched.
    let before = bill.clone();
    assert_eq!(
        bill.add(&catalog, "v-tee-l", 1),
        Err(BillError::InsufficientStock {
            variant_id: "v-tee-l".to_owned(),
            available: 0,
            requested: 1,
        })
    );
    assert_eq!(
        bill.add(&catalog, "v-tee-s", 0),
        Err(BillError::NonPositiveQuantity {
            variant_id: "v-tee-s".to_owned(),
        })
    );
    assert_eq!(
        bill.add(&catalog, "missing", 1),
        Err(BillError::UnknownVariant("missing".to_owned()))
    );
    assert_eq!(bill, before, "a rejected change must not alter the bill");

    // Clearing empties the bill and its subtotal.
    bill.clear();
    assert!(bill.is_empty());
    assert_eq!(bill.subtotal_minor(), 0);
    assert_eq!(
        format_subtotal(bill.subtotal_minor(), bill.currency()),
        "0.00"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bill_contract_holds_natively() {
    assert_bill_contract();
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn bill_contract_holds_in_wasm() {
    assert_bill_contract();
}
