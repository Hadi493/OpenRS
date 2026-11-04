//! Cross-target catalog contract.
//!
//! One assertion body runs twice: as a plain `#[test]` under
//! `cargo test -p openrs-core`, and as a `wasm_bindgen_test` in Chromium under
//! `wasm-pack test --headless --chrome crates/openrs-core`. Both shells list the
//! catalog with this same code, so a divergence between native and web would
//! change what a user sees and must fail here.

use openrs_core::{Catalog, StockStatus, format_price};

/// Listing and pricing rules every target must reproduce.
fn assert_catalog_contract() {
    let catalog = Catalog::sample();

    let names: Vec<&str> = catalog
        .products()
        .iter()
        .map(|product| product.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "Ceramic Mug",
            "Cotton T-Shirt",
            "Espresso Beans 250 g",
            "Notebook A5"
        ],
        "product order must be identical on every target"
    );

    let shirt = catalog.find("shirt", None);
    assert_eq!(shirt.len(), 1);
    assert_eq!(shirt[0].variants.len(), 3, "every size is listed");

    let sku_match = catalog.find("tee-m", None);
    assert_eq!(sku_match.len(), 1);
    let skus: Vec<&str> = sku_match[0]
        .variants
        .iter()
        .map(|variant| variant.sku.as_str())
        .collect();
    assert_eq!(skus, vec!["TEE-M"], "a SKU query narrows the variants");

    assert_eq!(
        catalog.find("", Some("Grocery")).len(),
        1,
        "category filtering is exact and case-insensitive"
    );
    assert!(catalog.find("spaceship", None).is_empty());

    let out_of_stock: Vec<&str> = catalog
        .variants()
        .iter()
        .filter(|variant| variant.stock_status() == StockStatus::OutOfStock)
        .map(|variant| variant.sku.as_str())
        .collect();
    assert_eq!(out_of_stock, vec!["MUG-WHT", "TEE-L"]);

    assert_eq!(format_price(1_499, "USD"), "14.99 USD");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn catalog_contract_holds_natively() {
    assert_catalog_contract();
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn catalog_contract_holds_in_wasm() {
    assert_catalog_contract();
}
