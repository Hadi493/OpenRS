//! WebAssembly parity test for the shared foundation.
//!
//! Run with `wasm-pack test --headless --chrome crates/openrs-core`.

#![cfg(target_arch = "wasm32")]

use openrs_core::{PRODUCT_NAME, foundation_message};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn shared_foundation_contract_runs_in_wasm() {
    assert_eq!(PRODUCT_NAME, "OpenRS");
    assert!(foundation_message().contains(PRODUCT_NAME));
}
