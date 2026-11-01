//! Native contract tests for the shared foundation.

use openrs_core::{PRODUCT_NAME, foundation_message};

#[test]
fn shared_foundation_contract_is_stable() {
    assert_eq!(PRODUCT_NAME, "OpenRS");
    assert!(foundation_message().contains(PRODUCT_NAME));
}
