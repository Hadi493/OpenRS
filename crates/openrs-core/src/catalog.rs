//! Product catalog: the read side of the catalog domain.
//!
//! Phase 1 of `ROADMAP.md` is a **read-only** catalog: products and variants
//! come from one immutable [`Catalog`] snapshot and the UI only lists them.
//! Creating, editing, and deactivating catalog entries is not included, because
//! product-management scope is still an open decision in `README.md`.
//!
//! Everything here is pure Rust with no platform calls, so the desktop app and
//! the web app list exactly the same data. Sample data lives here too, in
//! [`Catalog::sample`], so both shells can show a catalog before the catalog is
//! persisted (see `docs/phase-1-catalog-report.md`).

use std::collections::BTreeSet;

/// Exact money amount in integer minor units (for example, cents).
///
/// `SPEC.md` forbids floating point for money, so every amount is an `i64`
/// count of minor units paired with a currency code.
pub type MinorUnits = i64;

/// How many minor units make one currency unit.
///
/// Every supported currency is assumed to have two decimal places. Currencies
/// with zero (JPY) or three (KWD) decimals need a per-currency table and a
/// project-owner decision, so they are deliberately not supported yet.
pub const MINOR_UNITS_PER_UNIT: i64 = 100;

/// A product, matching the `Product` section of `SPEC.md`.
///
/// The `active` flag from the spec is not modelled yet: nothing can deactivate
/// a product until catalog management exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    /// Stable product identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Flat category label. Hierarchy is not part of the current scope.
    pub category: String,
}

/// A sellable variant of a product.
///
/// Variant attributes such as size or color are currently carried only by the
/// SKU, because the storage schema has no attribute column yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// Stable variant identifier.
    pub id: String,
    /// Owning product identifier.
    pub product_id: String,
    /// SKU or barcode. Unique when present.
    pub sku: String,
    /// Unit price in minor units.
    pub price_minor: MinorUnits,
    /// Currency code for [`Self::price_minor`].
    pub currency: String,
    /// Available stock. Never negative in stored data.
    pub stock: i64,
}

/// Whether a variant can be sold right now.
///
/// Low-stock warnings are Phase 5 and are intentionally not a state here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockStatus {
    /// At least one unit is available.
    Available,
    /// Stock is zero or less.
    OutOfStock,
}

impl Variant {
    /// Returns the availability of this variant.
    #[must_use]
    pub const fn stock_status(&self) -> StockStatus {
        if self.stock > 0 {
            StockStatus::Available
        } else {
            StockStatus::OutOfStock
        }
    }

    /// Returns `true` when the variant cannot be sold.
    ///
    /// The UI uses this to make unavailable variants visibly unavailable and
    /// non-selectable, as required by `SPEC.md` and `UI-UX.md`.
    #[must_use]
    pub const fn is_out_of_stock(&self) -> bool {
        matches!(self.stock_status(), StockStatus::OutOfStock)
    }
}

/// A product together with the variants that match a listing query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry<'a> {
    /// The product being listed.
    pub product: &'a Product,
    /// Matching variants, in catalog order. May be empty for a product that has
    /// no variants at all.
    pub variants: Vec<&'a Variant>,
}

/// An immutable catalog snapshot.
///
/// The snapshot is ordered once at construction so listing is deterministic:
/// products sort by name (case-insensitive), then id; variants sort by SKU,
/// then id. That order is what the UI renders and what tests assert, so both
/// targets show the same rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    products: Vec<Product>,
    variants: Vec<Variant>,
}

impl Catalog {
    /// Builds a catalog snapshot from products and variants.
    ///
    /// Both inputs are reordered deterministically. Duplicate ids are not
    /// rejected here; storage enforces uniqueness when the catalog is loaded
    /// from a database.
    #[must_use]
    pub fn new(products: Vec<Product>, variants: Vec<Variant>) -> Self {
        let mut products = products;
        let mut variants = variants;

        products.sort_by_cached_key(|product| (product.name.to_lowercase(), product.id.clone()));
        variants.sort_by_cached_key(|variant| (variant.sku.clone(), variant.id.clone()));

        Self { products, variants }
    }

    /// Returns the sample catalog used by both shells during development.
    ///
    /// It covers the states Phase 1 must show: several categories, products
    /// with multiple variants, and one fully out-of-stock product.
    #[must_use]
    pub fn sample() -> Self {
        let product = |id: &str, name: &str, category: &str| Product {
            id: id.to_owned(),
            name: name.to_owned(),
            category: category.to_owned(),
        };
        let variant = |id: &str, product_id: &str, sku: &str, price: i64, stock: i64| Variant {
            id: id.to_owned(),
            product_id: product_id.to_owned(),
            sku: sku.to_owned(),
            price_minor: price,
            currency: "USD".to_owned(),
            stock,
        };

        Self::new(
            vec![
                product("p-tee", "Cotton T-Shirt", "Apparel"),
                product("p-mug", "Ceramic Mug", "Homeware"),
                product("p-beans", "Espresso Beans 250 g", "Grocery"),
                product("p-notebook", "Notebook A5", "Stationery"),
            ],
            vec![
                variant("v-tee-s", "p-tee", "TEE-S", 1_499, 12),
                variant("v-tee-m", "p-tee", "TEE-M", 1_499, 7),
                variant("v-tee-l", "p-tee", "TEE-L", 1_599, 0),
                variant("v-mug-white", "p-mug", "MUG-WHT", 1_250, 0),
                variant("v-mug-grey", "p-mug", "MUG-GRY", 1_250, 5),
                variant("v-beans", "p-beans", "BEAN-250", 1_899, 24),
                variant("v-notebook", "p-notebook", "NOTE-A5", 850, 40),
            ],
        )
    }

    /// Returns every product in catalog order.
    #[must_use]
    pub fn products(&self) -> &[Product] {
        &self.products
    }

    /// Returns every variant in catalog order, across all products.
    #[must_use]
    pub fn variants(&self) -> &[Variant] {
        &self.variants
    }

    /// Returns the sorted, de-duplicated category labels.
    #[must_use]
    pub fn categories(&self) -> Vec<&str> {
        self.products
            .iter()
            .map(|product| product.category.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Lists products for the catalog screen.
    ///
    /// `query` is trimmed and matched case-insensitively against the product
    /// name, its category, and its SKU codes. An empty query lists everything.
    /// `category` filters by exact label (case-insensitive); `None` means every
    /// category.
    ///
    /// A product is included when its name, category, or any of its SKUs
    /// matches. A name or category match lists all of that product's variants;
    /// a SKU match lists only the matching variants. Products whose variants all
    /// fail the query are omitted.
    ///
    /// The scan is linear, which is intentional: a single store's catalog is
    /// small, and an index would need to be rebuilt on every keystroke.
    #[must_use]
    pub fn find(&self, query: &str, category: Option<&str>) -> Vec<CatalogEntry<'_>> {
        let needle = query.trim().to_lowercase();

        self.products
            .iter()
            .filter(|product| {
                category.is_none_or(|wanted| product.category.eq_ignore_ascii_case(wanted))
            })
            .filter_map(|product| {
                let product_matches = needle.is_empty()
                    || contains(&product.name, &needle)
                    || contains(&product.category, &needle);

                let variants: Vec<&Variant> = self
                    .variants
                    .iter()
                    .filter(|variant| variant.product_id == product.id)
                    .filter(|variant| product_matches || contains(&variant.sku, &needle))
                    .collect();

                if variants.is_empty() && !product_matches {
                    return None;
                }

                Some(CatalogEntry { product, variants })
            })
            .collect()
    }
}

/// Case-insensitive substring test. `needle` is already lowercased.
fn contains(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(needle)
}

/// Formats an exact amount for display, for example `12.50 USD`.
///
/// Parentheses-free negative amounts keep the minus sign so a mistaken negative
/// price is visible instead of being rendered as a positive one.
#[must_use]
pub fn format_price(amount: MinorUnits, currency: &str) -> String {
    let magnitude = i128::from(amount).abs();
    let units = magnitude / i128::from(MINOR_UNITS_PER_UNIT);
    let minor = magnitude % i128::from(MINOR_UNITS_PER_UNIT);
    let sign = if amount < 0 { "-" } else { "" };

    if currency.is_empty() {
        format!("{sign}{units}.{minor:02}")
    } else {
        format!("{sign}{units}.{minor:02} {currency}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Catalog, StockStatus, Variant, format_price};

    fn skus(catalog: &Catalog, query: &str, category: Option<&str>) -> Vec<Vec<String>> {
        catalog
            .find(query, category)
            .iter()
            .map(|entry| entry.variants.iter().map(|v| v.sku.clone()).collect())
            .collect()
    }

    #[test]
    fn sample_catalog_is_ordered_and_complete() {
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
            ]
        );
        assert_eq!(catalog.variants().len(), 7);
        assert_eq!(
            catalog.categories(),
            vec!["Apparel", "Grocery", "Homeware", "Stationery"]
        );
    }

    #[test]
    fn empty_query_lists_every_product_with_its_variants() {
        let catalog = Catalog::sample();
        let entries = catalog.find("", None);

        assert_eq!(entries.len(), 4);
        let tee = entries
            .iter()
            .find(|entry| entry.product.id == "p-tee")
            .expect("the t-shirt should be listed");
        assert_eq!(tee.variants.len(), 3, "all sizes should be listed");
    }

    #[test]
    fn query_is_trimmed_and_case_insensitive() {
        let catalog = Catalog::sample();

        assert_eq!(catalog.find("  shirt  ", None).len(), 1);
        assert_eq!(catalog.find("SHIRT", None).len(), 1);
        assert_eq!(
            skus(&catalog, "shirt", None),
            vec![vec!["TEE-L", "TEE-M", "TEE-S"]]
        );
    }

    #[test]
    fn a_sku_query_narrows_to_matching_variants() {
        let catalog = Catalog::sample();

        assert_eq!(
            skus(&catalog, "tee-m", None),
            vec![vec!["TEE-M".to_owned()]],
            "only the matching variant should be listed"
        );
        assert_eq!(
            catalog.find("tee-m", None)[0].product.id,
            "p-tee",
            "the owning product should still be shown"
        );
    }

    #[test]
    fn category_is_matched_by_name_and_filters_exactly() {
        let catalog = Catalog::sample();

        assert_eq!(catalog.find("apparel", None).len(), 1, "category matches");
        assert_eq!(catalog.find("", Some("Homeware")).len(), 1);
        assert_eq!(
            catalog.find("", Some("homeware")).len(),
            1,
            "case-insensitive"
        );
        assert!(
            catalog.find("", Some("Toys")).is_empty(),
            "an unknown category lists nothing"
        );
        assert!(
            catalog.find("shirt", Some("Grocery")).is_empty(),
            "the category filter and the query combine with AND"
        );
    }

    #[test]
    fn unknown_query_lists_nothing() {
        let catalog = Catalog::sample();
        assert!(catalog.find("spaceship", None).is_empty());
    }

    #[test]
    fn stock_status_reflects_availability() {
        let catalog = Catalog::sample();
        let tee_l = catalog
            .variants()
            .iter()
            .find(|variant| variant.sku == "TEE-L")
            .expect("sample data should contain TEE-L");
        let tee_s = catalog
            .variants()
            .iter()
            .find(|variant| variant.sku == "TEE-S")
            .expect("sample data should contain TEE-S");

        assert_eq!(tee_l.stock_status(), StockStatus::OutOfStock);
        assert!(tee_l.is_out_of_stock());
        assert_eq!(tee_s.stock_status(), StockStatus::Available);
        assert!(!tee_s.is_out_of_stock());
    }

    #[test]
    fn negative_stock_counts_as_out_of_stock() {
        let variant = Variant {
            id: "v".to_owned(),
            product_id: "p".to_owned(),
            sku: "SKU".to_owned(),
            price_minor: 100,
            currency: "USD".to_owned(),
            stock: -3,
        };

        assert!(variant.is_out_of_stock());
    }

    #[test]
    fn prices_format_with_exact_minor_units() {
        assert_eq!(format_price(1_499, "USD"), "14.99 USD");
        assert_eq!(format_price(0, "USD"), "0.00 USD");
        assert_eq!(format_price(5, "USD"), "0.05 USD");
        assert_eq!(format_price(1_000, ""), "10.00");
        assert_eq!(format_price(-1_250, "USD"), "-12.50 USD");
        assert_eq!(
            format_price(i64::MIN, "USD"),
            "-92233720368547758.08 USD",
            "the extreme value must not overflow while formatting"
        );
    }

    #[test]
    fn products_without_variants_are_listed() {
        let product = super::Product {
            id: "p-empty".to_owned(),
            name: "Empty Product".to_owned(),
            category: "Misc".to_owned(),
        };
        let catalog = Catalog::new(vec![product], Vec::new());

        let entries = catalog.find("", None);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].variants.is_empty());
    }
}
