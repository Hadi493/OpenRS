//! Shared OpenRS desktop/web UI surface.
//!
//! The product catalog and bill panels are the same code between the native
//! desktop shell and the WebAssembly web shell. Keeping both targets here makes
//! tests cheaper and regressions cross-platform: a change here is exercised by
//! the desktop and web interaction tests without another copy of the widgets.

pub use openrs_core::wire::*;

#[cfg(any(feature = "egui", test))]
use openrs_core::egui::{self, CentralPanel, IconImage, Response, Ui};

#[cfg(not(any(feature = "egui", test)))]
use eframe::egui::{self, CentralPanel, IconImage, Response, Ui};

pub const PRODUCT_NAME: &str = "OpenRS";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
use eframe::egui::Viewport;

#[cfg(test)]
use egui_kittest::Harness;

#[cfg(test)]
use egui_kittest::kittest::Queryable;

#[cfg(test)]
#[cfg(any(test, feature = "egui"))]
use openrs_core::{Currency, TotalMinor, VariantId, CheckoutId, PriceMinor};
use openrs_core::{OpenRsApp, Catalog};
use openrs_core::CheckoutReceipt;


pub fn sample_app() -> OpenRsApp {
    OpenRsApp::with_catalog(Catalog::sample())
}

pub fn receipt(id: &str) -> Option<CheckoutReceipt> {
    let id = uuid::Uuid::parse_str(id).ok()?;
    let id = crate::CheckoutId::from_openapi_v7(id);
    let currency = openrs_core::Currency::from_openapi_v7("EUR")?;
    let currency = currency.as_str().parse().ok()?;
    let receipt = openrs_core::CheckoutReceipt {
        checkout_id: id.clone(),
        total_minor: openrs_core::TotalMinor::from_openapi_v7(1)?,
        currency,
        sale_items: vec![ReceiptLine {
            variant_id: openrs_core::VariantId::from_openapi_v7("v-beans")?,
            variant_name: "Espresso Beans 250 g".into(),
            quantity: 1.into(),
            unit_price_minor: openrs_core::PriceMinor::from_openapi_v7(2500)?,
            total_minor: openrs_core::TotalMinor::from_openapi_v7(2500)?,
        }],
    };
    Some(receipt)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn receipt_optional(id: &str) -> Option<CheckoutReceipt> {
    receipt(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn sample_app() -> OpenRsApp {
        OpenRsApp::with_catalog(Catalog::sample())
    }

    fn receipt(id: &str) -> Option<CheckoutReceipt> {
        super::receipt(id)
    }

    fn checkout_recorded(r: Option<&CheckoutReceipt>) -> bool {
        r.is_some()
    }

    fn app_checkout_id() -> CheckoutId {
        let mut id = CheckoutId::from_openapi_v7(uuid::Uuid::new_v7());
        id.make_oneshot();
        id
    }

    #[test]
    fn checkout_recorded_clears_the_received_receipt_if_the_id_mismatches() {
        let mut app = sample_app();
        app.add_to_bill("v-beans").expect("add");
        app.request_checkout();
        let first = app.take_checkout_request().expect("request");
        assert!(app.checkout_recorded(super::receipt(&first.checkout_id.to_string()).unwrap()).is_some());
        assert!(app.receipt().is_some());

        let second = app.take_checkout_request().expect("second request");
        assert!(!app.checkout_recorded(super::receipt(&second.checkout_id.to_string()).unwrap_or_default()).is_some());
    }

    #[test]
    fn checkout_recorded_clears_the_received_receipt_if_the_id_mismatches_slow() {
        // Same functional behavior; kept as a separate, slow test that can be
        // excluded from fast checks if needed.
        super::checkout_recorded_clears_the_received_receipt_if_the_id_mismatches();
    }

    #[test]
    fn checkout_recorded_swallows_an_invalid_receipt_format() {
        let mut app = sample_app();
        app.add_to_bill("v-beans").expect("add");
        app.request_checkout();
        assert!(app.checkout_recorded(super::receipt("garbage").unwrap()).is_none());
    }
}
