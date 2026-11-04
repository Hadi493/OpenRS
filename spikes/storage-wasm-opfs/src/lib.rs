//! Browser persistence spike for OpenRS.
//!
//! The sibling `storage-wasm` spike proves that SQLite **runs** in a browser and
//! enforces the `SPEC.md` schema. It uses `:memory:` only, so it says nothing
//! about whether data survives a page reload. This crate closes that gap by
//! installing the OPFS `sahpool` VFS and storing a database in the origin
//! private file system.
//!
//! Run with:
//!
//! ```bash
//! wasm-pack test --headless --chrome spikes/storage-wasm-opfs
//! ```
//!
//! Everything here is `unsafe` FFI, for the same reason as the other WASM
//! spike: `sqlite-wasm-rs` exposes the C API directly.
//!
//! # Why a separate crate
//!
//! OPFS sync access handles only exist in a dedicated worker, so the tests must
//! be configured with `run_in_dedicated_worker`. `wasm-bindgen-test` allows one
//! mode per crate, and the sibling spike already uses `run_in_browser`.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::wasm_bindgen;

/// Build marker, so the crate produces a `.wasm` module that `wasm-pack build`
/// can measure with and without the `storage` feature.
#[wasm_bindgen]
#[must_use]
pub fn spike_marker() -> String {
    String::from("openrs-storage-opfs-spike")
}

/// Installs the OPFS VFS and reports the SQLite version it is running.
///
/// This is the only non-test entry point that pulls in the storage stack. It
/// exists so the storage feature has a measured WebAssembly size instead of an
/// extrapolated one; the tests do not link into a `wasm-pack build` artifact.
#[cfg(feature = "storage")]
#[wasm_bindgen]
pub async fn install_spike_storage() -> Result<String, wasm_bindgen::JsValue> {
    use sqlite_wasm_rs::WasmOsCallback;
    use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfgBuilder, install};

    let config = OpfsSAHPoolCfgBuilder::new()
        .directory("openrs-spike")
        .build();
    install::<WasmOsCallback>(&config, true)
        .await
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;

    // SAFETY: `sqlite3_libversion` returns a static NUL-terminated string that
    // SQLite owns for the lifetime of the process.
    let version = unsafe {
        std::ffi::CStr::from_ptr(sqlite_wasm_rs::sqlite3_libversion())
            .to_string_lossy()
            .into_owned()
    };
    Ok(version)
}

#[cfg(all(test, feature = "storage"))]
mod tests {
    use std::ffi::{CStr, CString, c_int};
    use std::ptr;

    use sqlite_wasm_rs::{WasmOsCallback, sqlite3, sqlite3_close, sqlite3_open_v2};
    use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfgBuilder, install};
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    wasm_bindgen_test_configure!(run_in_dedicated_worker);

    const DATABASE: &str = "openrs-opfs-spike.db";
    const VFS_NAME: &str = "opfs-sahpool";

    /// Owns a SQLite handle and closes it on drop.
    struct Database(*mut sqlite3);

    impl Database {
        fn open(name: &str) -> Self {
            let name = CString::new(name).expect("no interior nul");
            let mut handle: *mut sqlite3 = ptr::null_mut();
            // SAFETY: `handle` is a valid out-pointer, `name` is a valid C
            // string that outlives the call, and the VFS argument is null so
            // SQLite uses whatever VFS was installed as the default.
            let code = unsafe {
                sqlite3_open_v2(
                    name.as_ptr(),
                    &raw mut handle,
                    sqlite_wasm_rs::SQLITE_OPEN_READWRITE | sqlite_wasm_rs::SQLITE_OPEN_CREATE,
                    ptr::null(),
                )
            };
            assert_eq!(code, sqlite_wasm_rs::SQLITE_OK, "open failed with {code}");
            Self(handle)
        }

        fn exec(&self, sql: &str) -> c_int {
            let sql = CString::new(sql).expect("no interior nul");
            // SAFETY: the handle is live for the lifetime of `self`, `sql` is a
            // valid C string, and no callback or error buffer is requested.
            unsafe {
                sqlite_wasm_rs::sqlite3_exec(
                    self.0,
                    sql.as_ptr(),
                    None,
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            }
        }

        fn exec_ok(&self, sql: &str) {
            let code = self.exec(sql);
            assert_eq!(
                code,
                sqlite_wasm_rs::SQLITE_OK,
                "statement failed ({code}): {sql}"
            );
        }

        /// Returns the first column of the first row as an integer.
        fn scalar_i64(&self, sql: &str) -> i64 {
            let sql_text = sql;
            let sql = CString::new(sql).expect("no interior nul");
            let mut statement: *mut sqlite_wasm_rs::sqlite3_stmt = ptr::null_mut();
            // SAFETY: all pointers are valid out-pointers; `-1` asks SQLite to
            // read up to the NUL terminator.
            let code = unsafe {
                sqlite_wasm_rs::sqlite3_prepare_v2(
                    self.0,
                    sql.as_ptr(),
                    -1,
                    &raw mut statement,
                    ptr::null_mut(),
                )
            };
            assert_eq!(
                code,
                sqlite_wasm_rs::SQLITE_OK,
                "prepare failed ({code}): {sql_text}"
            );

            // SAFETY: `statement` was produced by a successful prepare and is
            // finalised below.
            let value = unsafe {
                let step = sqlite_wasm_rs::sqlite3_step(statement);
                assert_eq!(
                    step,
                    sqlite_wasm_rs::SQLITE_ROW,
                    "expected a row for: {sql_text}"
                );
                sqlite_wasm_rs::sqlite3_column_int64(statement, 0)
            };

            // SAFETY: `statement` is finalised exactly once here.
            unsafe {
                sqlite_wasm_rs::sqlite3_finalize(statement);
            }
            value
        }

        /// Returns the first column of the first row as text.
        fn scalar_text(&self, sql: &str) -> String {
            let sql_text = sql;
            let sql = CString::new(sql).expect("no interior nul");
            let mut statement: *mut sqlite_wasm_rs::sqlite3_stmt = ptr::null_mut();
            // SAFETY: all pointers are valid out-pointers; `-1` asks SQLite to
            // read up to the NUL terminator.
            let code = unsafe {
                sqlite_wasm_rs::sqlite3_prepare_v2(
                    self.0,
                    sql.as_ptr(),
                    -1,
                    &raw mut statement,
                    ptr::null_mut(),
                )
            };
            assert_eq!(
                code,
                sqlite_wasm_rs::SQLITE_OK,
                "prepare failed ({code}): {sql_text}"
            );

            // SAFETY: `statement` was produced by a successful prepare and is
            // finalised below. The text pointer is copied before finalising.
            let value = unsafe {
                let step = sqlite_wasm_rs::sqlite3_step(statement);
                assert_eq!(
                    step,
                    sqlite_wasm_rs::SQLITE_ROW,
                    "expected a row for: {sql_text}"
                );
                let text = sqlite_wasm_rs::sqlite3_column_text(statement, 0);
                CStr::from_ptr(text.cast()).to_string_lossy().into_owned()
            };

            // SAFETY: `statement` is finalised exactly once here.
            unsafe {
                sqlite_wasm_rs::sqlite3_finalize(statement);
            }
            value
        }

        fn error_message(&self) -> String {
            // SAFETY: the handle is live; the returned pointer is owned by
            // SQLite and must not be freed by us.
            unsafe {
                let message = sqlite_wasm_rs::sqlite3_errmsg(self.0);
                if message.is_null() {
                    return String::from("<no message>");
                }
                CStr::from_ptr(message).to_string_lossy().into_owned()
            }
        }

        fn close(self) -> c_int {
            // `close` takes the handle by value, so `Drop` will not run. The
            // `ManuallyDrop` dance is avoided by closing and forgetting.
            let code = unsafe { sqlite3_close(self.0) };
            std::mem::forget(self);
            code
        }
    }

    impl Drop for Database {
        fn drop(&mut self) {
            // SAFETY: the handle came from `sqlite3_open_v2` and is closed once
            // unless `close` already consumed it.
            unsafe {
                sqlite3_close(self.0);
            }
        }
    }

    /// Installs the OPFS VFS as SQLite's default.
    ///
    /// This must be awaited: the install talks to the file system through
    /// JavaScript promises, which only progress on the worker's event loop.
    async fn install_opfs_vfs() {
        let config = OpfsSAHPoolCfgBuilder::new()
            .directory("openrs-spike")
            .clear_on_init(true)
            .build();
        install::<WasmOsCallback>(&config, true)
            .await
            .expect("install the OPFS VFS");
    }

    #[wasm_bindgen_test]
    async fn opfs_vfs_installs_and_registers() {
        install_opfs_vfs().await;

        let name = CString::new(VFS_NAME).expect("no interior nul");
        // SAFETY: `name` is a valid C string; `sqlite3_vfs_find` only reads it.
        let vfs = unsafe { sqlite_wasm_rs::sqlite3_vfs_find(name.as_ptr()) };
        assert!(!vfs.is_null(), "the OPFS VFS must be registered");
    }

    #[wasm_bindgen_test]
    async fn a_database_written_to_opfs_survives_a_reconnect() {
        install_opfs_vfs().await;

        let database = Database::open(DATABASE);
        database.exec_ok(
            "DROP TABLE IF EXISTS probe;
             CREATE TABLE probe (value INTEGER NOT NULL);
             INSERT INTO probe (value) VALUES (7);",
        );
        // Closing forces the data out to OPFS; a memory-only database would be
        // gone after this point.
        assert_eq!(
            database.close(),
            sqlite_wasm_rs::SQLITE_OK,
            "close must flush the database"
        );

        let reopened = Database::open(DATABASE);
        assert_eq!(
            reopened.scalar_i64("SELECT value FROM probe"),
            7,
            "the row written through the OPFS VFS must still be there: {}",
            reopened.error_message()
        );
    }

    /// The native repository uses WAL. The OPFS VFS documents that WAL is not
    /// supported, so this test records what actually happens instead of
    /// assuming the native pragmas can be reused verbatim.
    #[wasm_bindgen_test]
    async fn opfs_does_not_support_wal() {
        install_opfs_vfs().await;

        let database = Database::open("openrs-journal-spike.db");
        let mode = database.scalar_text("PRAGMA journal_mode = WAL");
        assert_eq!(
            mode, "delete",
            "OPFS must fall back to the rollback journal, got `{mode}`"
        );

        // Whatever the mode, the database must still be writable.
        database.exec_ok(
            "CREATE TABLE IF NOT EXISTS journal_probe (value INTEGER NOT NULL);
             INSERT INTO journal_probe (value) VALUES (1);",
        );
        assert_eq!(database.scalar_i64("SELECT COUNT(*) FROM journal_probe"), 1);
    }

    #[wasm_bindgen_test]
    async fn the_product_schema_works_on_opfs() {
        install_opfs_vfs().await;

        let schema = include_str!("../../storage/migrations/0001_init.sql");
        let database = Database::open("openrs-schema-spike.db");
        database.exec_ok(schema);
        assert_eq!(
            database.scalar_i64(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
                 AND name IN ('products', 'variants', 'sales', 'sale_items')"
            ),
            4,
            "all four tables must exist on the OPFS database"
        );
    }
}
