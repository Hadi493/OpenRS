-- Initial schema for OpenRS.
--
-- Design notes that matter for the SPEC guarantees:
--
-- * `variants.stock` has a CHECK (stock >= 0) so a bug can never persist a
--   negative stock count, even if application logic is wrong.
-- * `sales.checkout_id` is UNIQUE. This is the durable idempotency key: a
--   retried checkout collides here instead of creating a second sale.
-- * `sale_items` copies `sku`, `product_name`, `unit_price_minor`, and
--   `currency` at checkout time. A later price edit cannot rewrite history
--   because the sale never joins back to `variants` for display.

PRAGMA foreign_keys = ON;

CREATE TABLE products (
    id       TEXT PRIMARY KEY,
    name     TEXT NOT NULL,
    category TEXT NOT NULL,
    active   INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1))
);

CREATE TABLE variants (
    id          TEXT PRIMARY KEY,
    product_id  TEXT NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    sku         TEXT NOT NULL UNIQUE,
    price_minor INTEGER NOT NULL CHECK (price_minor >= 0),
    currency    TEXT NOT NULL,
    stock       INTEGER NOT NULL CHECK (stock >= 0)
);

CREATE TABLE sales (
    id             TEXT PRIMARY KEY,
    checkout_id    TEXT NOT NULL UNIQUE,
    created_at     TEXT NOT NULL,
    currency       TEXT NOT NULL,
    subtotal_minor INTEGER NOT NULL CHECK (subtotal_minor >= 0),
    total_minor    INTEGER NOT NULL CHECK (total_minor >= 0),
    status         TEXT NOT NULL CHECK (status IN ('completed'))
);

CREATE TABLE sale_items (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    sale_id          TEXT NOT NULL REFERENCES sales (id) ON DELETE CASCADE,
    variant_id       TEXT NOT NULL,
    sku              TEXT NOT NULL,
    product_name     TEXT NOT NULL,
    unit_price_minor INTEGER NOT NULL CHECK (unit_price_minor >= 0),
    currency         TEXT NOT NULL,
    quantity         INTEGER NOT NULL CHECK (quantity > 0),
    line_total_minor INTEGER NOT NULL CHECK (line_total_minor >= 0)
);

CREATE INDEX sale_items_sale_id ON sale_items (sale_id);
CREATE INDEX sales_created_at ON sales (created_at);
