//! Browser-side storage spike.
//!
//! Answers three questions the native spike cannot:
//!
//! 1. Does the SQLite C library compile and run under `wasm32-unknown-unknown`?
//! 2. Do the same DDL, the same transaction, and the same `UNIQUE` idempotency
//!    constraint behave identically in a browser?
//! 3. Is the resulting module small enough to ship alongside `wgpu`?
//!
//! Run with:
//!
//! ```bash
//! wasm-pack test --headless --chrome spikes/storage-wasm
//! ```
//!
//! Everything here is `unsafe` FFI. That is the first finding: `sqlite-wasm-rs`
//! exposes the C API directly, so adopting it in the product requires a safe
//! wrapper layer that the project's `unsafe_code = "forbid"` rule permits.

#![cfg(target_arch = "wasm32")]

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString, c_int, c_void};
    use std::ptr;

    use sqlite_wasm_rs as ffi;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    wasm_bindgen_test_configure!(run_in_browser);

    /// Reuses the *same* schema file as the native spike.
    const SCHEMA: &str = include_str!("../../storage/migrations/0001_init.sql");

    /// Owns a SQLite handle and closes it on drop.
    struct Database(*mut ffi::sqlite3);

    impl Database {
        fn open_memory() -> Self {
            let name = CString::new(":memory:").expect("no interior nul");
            let mut handle: *mut ffi::sqlite3 = ptr::null_mut();
            // SAFETY: `handle` is a valid out-pointer, `name` is a valid C string
            // that outlives the call, and the VFS argument is explicitly null.
            let code = unsafe {
                ffi::sqlite3_open_v2(
                    name.as_ptr(),
                    &raw mut handle,
                    ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
                    ptr::null(),
                )
            };
            assert_eq!(code, ffi::SQLITE_OK, "sqlite3_open_v2 failed with {code}");
            Self(handle)
        }

        fn exec(&self, sql: &str) -> c_int {
            let sql = CString::new(sql).expect("no interior nul");
            // SAFETY: the handle is live for the lifetime of `self`, `sql` is a
            // valid C string, and no callback or error buffer is requested.
            unsafe {
                ffi::sqlite3_exec(self.0, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut())
            }
        }

        fn exec_ok(&self, sql: &str) {
            let code = self.exec(sql);
            assert_eq!(code, ffi::SQLITE_OK, "statement failed ({code}): {sql}");
        }

        /// Returns the first column of the first row as an integer.
        fn scalar_i64(&self, sql: &str) -> i64 {
            // Keep the readable SQL around for assertion messages: `CString`
            // does not implement `Display`.
            let sql_text = sql;
            let sql = CString::new(sql).expect("no interior nul");
            let mut statement: *mut ffi::sqlite3_stmt = ptr::null_mut();
            // SAFETY: all pointers are valid out-parameters for the duration of
            // the call; `-1` asks SQLite to read up to the NUL terminator.
            let code = unsafe {
                ffi::sqlite3_prepare_v2(
                    self.0,
                    sql.as_ptr(),
                    -1,
                    &raw mut statement,
                    ptr::null_mut(),
                )
            };
            assert_eq!(code, ffi::SQLITE_OK, "prepare failed ({code}): {sql_text}");

            // SAFETY: `statement` was produced by a successful prepare and is
            // finalised below.
            let value = unsafe {
                let step = ffi::sqlite3_step(statement);
                assert_eq!(step, ffi::SQLITE_ROW, "expected a row for: {sql_text}");
                ffi::sqlite3_column_int64(statement, 0)
            };

            // SAFETY: `statement` is finalised exactly once here.
            unsafe {
                ffi::sqlite3_finalize(statement);
            }
            value
        }

        fn error_message(&self) -> String {
            // SAFETY: the handle is live; the returned pointer is owned by
            // SQLite and must not be freed by us.
            unsafe {
                let message = ffi::sqlite3_errmsg(self.0);
                if message.is_null() {
                    return String::from("<no message>");
                }
                CStr::from_ptr(message).to_string_lossy().into_owned()
            }
        }
    }

    impl Drop for Database {
        fn drop(&mut self) {
            // SAFETY: the handle came from `sqlite3_open_v2` and is closed once.
            unsafe {
                ffi::sqlite3_close(self.0);
            }
        }
    }

    #[wasm_bindgen_test]
    fn sqlite_runs_in_the_browser() {
        let database = Database::open_memory();
        database.exec_ok("CREATE TABLE probe (value INTEGER NOT NULL)");
        database.exec_ok("INSERT INTO probe (value) VALUES (42)");
        assert_eq!(database.scalar_i64("SELECT value FROM probe"), 42);
    }

    #[wasm_bindgen_test]
    fn the_product_schema_is_accepted_by_browser_sqlite() {
        let database = Database::open_memory();
        database.exec_ok("PRAGMA foreign_keys = ON");
        database.exec_ok(SCHEMA);

        let tables = database.scalar_i64(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
             AND name IN ('products', 'variants', 'sales', 'sale_items')",
        );
        assert_eq!(tables, 4, "all four tables must exist");
    }

    #[wasm_bindgen_test]
    fn a_checkout_transaction_is_atomic_in_the_browser() {
        let database = Database::open_memory();
        database.exec_ok("PRAGMA foreign_keys = ON");
        database.exec_ok(SCHEMA);

        database.exec_ok(
            "INSERT INTO products (id, name, category) VALUES ('p1', 'Test T-Shirt', 'Apparel');
             INSERT INTO variants (id, product_id, sku, price_minor, currency, stock)
                 VALUES ('v1', 'p1', 'SKU-v1', 1999, 'USD', 10);",
        );

        // Commit a 3-unit sale.
        database.exec_ok("BEGIN IMMEDIATE");
        database.exec_ok(
            "INSERT INTO sales (id, checkout_id, created_at, currency, subtotal_minor,
                                total_minor, status)
             VALUES ('s1', 'checkout-1', '2026-11-02T00:00:00Z', 'USD', 5997, 5997, 'completed');
             INSERT INTO sale_items (sale_id, variant_id, sku, product_name, unit_price_minor,
                                     currency, quantity, line_total_minor)
             VALUES ('s1', 'v1', 'SKU-v1', 'Test T-Shirt', 1999, 'USD', 3, 5997);
             UPDATE variants SET stock = stock - 3 WHERE id = 'v1' AND stock >= 3;",
        );
        database.exec_ok("COMMIT");

        assert_eq!(
            database.scalar_i64("SELECT stock FROM variants WHERE id = 'v1'"),
            7
        );

        // A rolled-back checkout must change nothing.
        database.exec_ok("BEGIN IMMEDIATE");
        database.exec_ok("UPDATE variants SET stock = stock - 7 WHERE id = 'v1'");
        database.exec_ok("ROLLBACK");
        assert_eq!(
            database.scalar_i64("SELECT stock FROM variants WHERE id = 'v1'"),
            7,
            "rollback must restore stock"
        );

        // The CHECK constraint must refuse negative stock outright.
        assert_ne!(
            database.exec("UPDATE variants SET stock = -1 WHERE id = 'v1'"),
            ffi::SQLITE_OK,
            "CHECK (stock >= 0) must reject negative stock"
        );
    }

    #[wasm_bindgen_test]
    fn the_idempotency_key_is_enforced_in_the_browser() {
        let database = Database::open_memory();
        database.exec_ok(SCHEMA);
        database.exec_ok(
            "INSERT INTO sales (id, checkout_id, created_at, currency, subtotal_minor,
                                total_minor, status)
             VALUES ('s1', 'checkout-1', '2026-11-02T00:00:00Z', 'USD', 100, 100, 'completed')",
        );

        let code = database.exec(
            "INSERT INTO sales (id, checkout_id, created_at, currency, subtotal_minor,
                                total_minor, status)
             VALUES ('s2', 'checkout-1', '2026-11-02T00:00:01Z', 'USD', 100, 100, 'completed')",
        );

        // `sqlite3_exec` reports the primary result code (`SQLITE_CONSTRAINT`,
        // 19), not the extended one (`SQLITE_CONSTRAINT_UNIQUE`, 2067), so mask
        // off the extended bits before comparing.
        assert_eq!(
            code & 0xff,
            ffi::SQLITE_CONSTRAINT,
            "a duplicate checkout key must violate UNIQUE, got {code}: {}",
            database.error_message()
        );
        assert!(
            database.error_message().contains("sales.checkout_id"),
            "the constraint must name the idempotency column: {}",
            database.error_message()
        );
        assert_eq!(database.scalar_i64("SELECT COUNT(*) FROM sales"), 1);
    }

    #[wasm_bindgen_test]
    fn report_browser_storage_environment() {
        let global = js_sys::global();
        let isolated =
            js_sys::Reflect::get(&global, &js_sys::JsString::from("crossOriginIsolated"))
                .ok()
                .and_then(|value| value.as_bool())
                .unwrap_or(false);

        let has_opfs = js_sys::Reflect::has(&global, &js_sys::JsString::from("navigator"))
            .unwrap_or(false)
            && js_sys::Reflect::get(&global, &js_sys::JsString::from("navigator"))
                .ok()
                .and_then(|navigator| {
                    js_sys::Reflect::get(&navigator, &js_sys::JsString::from("storage")).ok()
                })
                .is_some();

        // Not assertions: this test exists to print the deployment constraints.
        web_sys::console::log_1(
            &format!("crossOriginIsolated={isolated} navigator_storage={has_opfs}").into(),
        );
    }
}
