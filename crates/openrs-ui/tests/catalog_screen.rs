//! Interaction tests for the shared catalog screen.
//!
//! These drive the real widgets through `egui_kittest`, so they check what a
//! user sees and types rather than only the catalog's own logic (which is
//! covered by `openrs-core`). The same shared UI code is what both shells
//! render, so passing here covers desktop and web layout-independent behavior.

use egui_kittest::Harness;
use egui_kittest::egui::Viewport;
use egui_kittest::kittest::Queryable;
use openrs_ui::OpenRsApp;

#[test]
fn typing_in_the_search_box_filters_the_list() {
    let mut app = OpenRsApp::with_catalog(openrs_core::Catalog::sample());
    let mut harness = Harness::new_ui(|ui| app.ui(ui));
    harness.run();

    let search = harness.get_by_role(egui::Role::TextInput);
    search.click();
    harness.run();

    let search = harness.get_by_role(egui::Role::TextInput);
    search.type_text("mug");
    harness.run();

    harness.get_by_label("Showing 1 of 4 products");
    harness.get_by_label("Ceramic Mug");
    assert!(
        harness.query_by_label("Cotton T-Shirt").is_none(),
        "products without a matching name, category, or SKU must disappear"
    );
}

#[test]
fn an_unmatched_search_shows_the_empty_state() {
    let mut app = OpenRsApp::with_catalog(openrs_core::Catalog::sample());
    app.set_query("spaceship");

    let mut harness = Harness::new_ui(move |ui| app.ui(ui));
    harness.run();

    harness.get_by_label("Showing 0 of 4 products");
    harness.get_by_label("No products match this search.");
    harness.get_by_label("Try a different name, category, or SKU.");
}

#[test]
fn a_selected_category_hides_other_categories() {
    let mut app = OpenRsApp::with_catalog(openrs_core::Catalog::sample());
    app.set_category(Some("Grocery"));

    let mut harness = Harness::new_ui(move |ui| app.ui(ui));
    harness.run();

    harness.get_by_label("Showing 1 of 4 products");
    harness.get_by_label("Espresso Beans 250 g");
    assert!(
        harness.query_by_label("Ceramic Mug").is_none(),
        "the category filter must hide other categories"
    );
}

#[test]
fn an_typed_quantity_is_committed_on_enter() {
    let mut app = OpenRsApp::with_catalog(openrs_core::Catalog::sample());
    let mut harness = Harness::new_ui(|ui| app.ui(ui));
    harness.run();

    let add = harness.get_by_label("Add p-beans to the bill").click();
    harness.run();

    let quantity = harness.get_by_label("Quantity for Espresso Beans 250 g");
    quantity.click();
    harness.run();
    quantity.type_text("2");
    harness.run();

    let confirm = harness.get_by_label("Confirm");
    harness.press_key(egui::Key::Enter);
    harness.run();

    let row = harness.get_by_label("Espresso Beans 250 g");
    assert!(
        row.text().contains("2.00") && row.text().contains("500"),
        "the line total should update after confirming a quantity"
    );
}

#[test]
fn clearing_the_search_restores_every_product() {
    let mut app = OpenRsApp::with_catalog(openrs_core::Catalog::sample());
    app.set_query("mug");

    let mut harness = Harness::new_ui(move |ui| app.ui(ui));
    harness.run();

    harness.get_by_label("Clear search").click();
    harness.run();

    harness.get_by_label("Showing 4 of 4 products");
    harness.get_by_label("Cotton T-Shirt");
}
