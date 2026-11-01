//! Shared egui user interface for OpenRS.
//!
//! The UI crate depends on `egui` only, not `eframe`. The desktop and web
//! shells provide the platform-specific windowing and rendering integration.
//! Keeping the UI independent of eframe makes it testable without opening a
//! window and keeps the platform shells small.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use egui::{CentralPanel, Ui};
use openrs_core::{PRODUCT_NAME, foundation_message};

/// Top-level shared application state and UI.
#[derive(Debug, Default)]
pub struct OpenRsApp;

impl OpenRsApp {
    /// Creates the shared application state.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Renders the application into the root egui `Ui`.
    ///
    /// `eframe::App::ui` supplies a root `Ui` without a margin or background,
    /// so the shared UI owns its central panel.
    pub fn ui(&mut self, ui: &mut Ui) {
        CentralPanel::default().show(ui, |ui| {
            ui.heading(PRODUCT_NAME);
            ui.label(foundation_message());
            ui.separator();
            ui.label("Phase 0.1 foundation smoke test");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::OpenRsApp;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use openrs_core::{PRODUCT_NAME, foundation_message};

    #[test]
    fn renders_the_shared_foundation_message() {
        let mut app = OpenRsApp::new();
        let mut harness = Harness::new_ui(|ui| app.ui(ui));

        harness.run();

        harness.get_by_label(PRODUCT_NAME);
        harness.get_by_label(foundation_message());
    }
}
