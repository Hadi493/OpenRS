//! Shared, platform-independent foundation for OpenRS.
//!
//! This crate must not depend on `egui`, `eframe`, filesystem APIs, or any
//! other platform-specific service. Both the native desktop app and the
//! WebAssembly web app link to this crate, so its behavior must be identical
//! on both targets.
//!
//! The catalog domain types ([`Product`], [`Variant`], [`Catalog`]) live here
//! rather than in `openrs-storage`, so that the UI, the sample data, and the
//! storage layer all speak about the same values. Storage re-exports them from
//! `openrs_storage::model`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod bill;
pub mod catalog;
pub mod checkout;
pub mod wire;

pub use bill::{Bill, BillError, BillLine, format_subtotal};
pub use checkout::{
    CheckoutLine, CheckoutReceipt, CheckoutRequest, new_checkout_id,
};
pub use catalog::{
    Catalog, CatalogEntry, MINOR_UNITS_PER_UNIT, MinorUnits, Product, StockStatus, Variant,
    format_price,
};
pub use wire::{
    CheckoutReply, WireError, WorkerMessage, catalog_message, checkout_failure_message,
    checkout_reply_message, checkout_request_message, decode_checkout_reply,
    decode_checkout_request, decode_message, decode_worker_message, error_message,
};

/// The product name used in the UI and user-facing documents.
pub const PRODUCT_NAME: &str = "OpenRS";

/// Returns a deterministic foundation message shared by every target.
///
/// This is intentionally trivial: Phase 0.1 only proves that the same Rust
/// code is compiled into the native and WebAssembly shells.
#[must_use]
pub const fn foundation_message() -> &'static str {
    "OpenRS foundation is running"
}

#[cfg(test)]
mod tests {
    use super::{PRODUCT_NAME, foundation_message};

    #[test]
    fn product_name_is_stable() {
        assert_eq!(PRODUCT_NAME, "OpenRS");
    }

    #[test]
    fn foundation_message_contains_product_name() {
        assert!(
            foundation_message().contains(PRODUCT_NAME),
            "the shared foundation message should name the product"
        );
    }
}
