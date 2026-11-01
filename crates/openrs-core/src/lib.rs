//! Shared, platform-independent foundation for OpenRS.
//!
//! This crate must not depend on `egui`, `eframe`, filesystem APIs, or any
//! other platform-specific service. Both the native desktop app and the
//! WebAssembly web app link to this crate, so its behavior must be identical
//! on both targets.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

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
