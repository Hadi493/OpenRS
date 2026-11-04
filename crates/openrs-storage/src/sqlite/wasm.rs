//! Browser SQLite driver over `sqlite-wasm-rs` and the OPFS VFS.
//!
//! # The audited `unsafe` exception
//!
//! ADR 0002, condition 4: this module is the only place in OpenRS where
//! `unsafe` is permitted, because `sqlite-wasm-rs` is a raw binding to the
//! SQLite C library and there is no safe alternative on `wasm32-unknown-unknown`.
//! The rest of the workspace keeps `unsafe_code = "forbid"`.
//!
//! Every `unsafe` block below is confined to this file and carries a SAFETY
//! comment. There is no other FFI surface: callers see [`Db`] through the same
//! [`SqlSession`] trait the native driver implements.
//!
//! # Platform notes
//!
//! - The database is opened on whatever VFS is installed as default: the
//!   in-memory VFS for tests, OPFS after [`install_opfs_vfs`].
//! - OPFS does **not** support WAL, so this driver never changes the journal
//!   mode. The schema and transaction model are otherwise identical to native.
//! - OPFS sync access handles only exist in a dedicated worker, and the VFS
//!   needs cross-origin isolation (COOP/COEP) for `SharedArrayBuffer`.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_int};
use std::ptr;

use sqlite_wasm_rs as ffi;
use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfgBuilder, install};

use super::driver::{Param, Row, SqlSession, Value};
use crate::model::RepoError;

/// One browser SQLite connection.
pub(crate) struct Db {
    handle: *mut ffi::sqlite3,
}

impl Db {
    /// Opens an in-memory database through the default (memory) VFS.
    pub(crate) fn open_in_memory() -> Result<Self, RepoError> {
        Self::open_name(":memory:")
    }

    /// Opens a database through the default VFS.
    ///
    /// After [`install_opfs_vfs`] this is an OPFS file; before it, the name is
    /// just an in-memory key.
    pub(crate) fn open(name: &str) -> Result<Self, RepoError> {
        Self::open_name(name)
    }

    fn open_name(name: &str) -> Result<Self, RepoError> {
        let name = CString::new(name)
            .map_err(|_| RepoError::Storage("database name contains a NUL byte".to_owned()))?;
        let mut handle: *mut ffi::sqlite3 = ptr::null_mut();

        // SAFETY: `handle` is a valid out-pointer, `name` is a valid NUL
        // terminated string that outlives the call, and the VFS argument is null
        // so SQLite uses the installed default.
        let code = unsafe {
            ffi::sqlite3_open_v2(
                name.as_ptr(),
                &raw mut handle,
                ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
                ptr::null(),
            )
        };

        if code != ffi::SQLITE_OK {
            let message = error_message(handle);
            close(handle);
            return Err(RepoError::Storage(message));
        }

        let db = Self { handle };
        db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        Ok(db)
    }

    /// Runs `f` inside one `IMMEDIATE` transaction.
    ///
    /// Same semantics as the native driver, same explicit rollback.
    pub(crate) fn transaction<T>(
        &mut self,
        f: impl FnOnce(&dyn SqlSession) -> Result<T, RepoError>,
    ) -> Result<T, RepoError> {
        self.execute_batch("BEGIN IMMEDIATE")?;

        match f(self) {
            Ok(value) => {
                self.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(error) => {
                let _ = self.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// Prepares one statement.
    fn prepare(&self, sql: &str) -> Result<Statement, RepoError> {
        let sql = CString::new(sql)
            .map_err(|_| RepoError::Storage("SQL contains a NUL byte".to_owned()))?;
        let mut statement: *mut ffi::sqlite3_stmt = ptr::null_mut();

        // SAFETY: `statement` is a valid out-pointer. SQLite copies the SQL
        // into the prepared statement, so `sql` may be dropped after the call.
        let code = unsafe {
            ffi::sqlite3_prepare_v2(
                self.handle,
                sql.as_ptr(),
                -1,
                &raw mut statement,
                ptr::null_mut(),
            )
        };

        if code != ffi::SQLITE_OK {
            return Err(self.error());
        }
        Ok(Statement { statement })
    }

    fn error(&self) -> RepoError {
        RepoError::Storage(error_message(self.handle))
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        close(self.handle);
    }
}

fn close(handle: *mut ffi::sqlite3) {
    if handle.is_null() {
        return;
    }
    // SAFETY: the handle came from `sqlite3_open_v2` and is closed exactly once:
    // either here or in the failure path before the `Db` is constructed.
    unsafe {
        ffi::sqlite3_close(handle);
    }
}

fn error_message(handle: *mut ffi::sqlite3) -> String {
    if handle.is_null() {
        return String::from("<no database handle>");
    }
    // SAFETY: the handle is live; the returned pointer is owned by SQLite and
    // is only read here.
    unsafe {
        let message = ffi::sqlite3_errmsg(handle);
        if message.is_null() {
            String::from("<no message>")
        } else {
            CStr::from_ptr(message).to_string_lossy().into_owned()
        }
    }
}

/// Installs the OPFS VFS as SQLite's default.
///
/// Idempotent for the same pool configuration: a second call reuses the
/// registered VFS, so reopening a database does not reset it.
pub(crate) async fn install_opfs_vfs() -> Result<(), RepoError> {
    let config = OpfsSAHPoolCfgBuilder::new().build();
    install::<ffi::WasmOsCallback>(&config, true)
        .await
        .map_err(|error| RepoError::Storage(error.to_string()))?;
    Ok(())
}

/// A prepared statement that is finalised on drop, including on early return.
struct Statement {
    statement: *mut ffi::sqlite3_stmt,
}

impl Drop for Statement {
    fn drop(&mut self) {
        // SAFETY: the statement came from `sqlite3_prepare_v2` and is finalised
        // exactly once here.
        unsafe {
            ffi::sqlite3_finalize(self.statement);
        }
    }
}

impl Statement {
    fn bind(&self, params: &[Param<'_>]) -> Result<(), RepoError> {
        for (index, param) in params.iter().enumerate() {
            let position = c_int::try_from(index + 1)
                .map_err(|_| RepoError::Storage("too many parameters".to_owned()))?;

            let code = match param {
                // SAFETY: the statement handle is live and the parameter index
                // is within the prepared statement's range.
                Param::Integer(value) => unsafe {
                    ffi::sqlite3_bind_int64(self.statement, position, *value)
                },
                Param::Text(text) => {
                    let text = CString::new(*text).map_err(|_| {
                        RepoError::Storage("parameter contains a NUL byte".to_owned())
                    })?;
                    // SAFETY: as above; `SQLITE_TRANSIENT` makes SQLite copy the
                    // text before `text` is dropped.
                    unsafe {
                        ffi::sqlite3_bind_text(
                            self.statement,
                            position,
                            text.as_ptr(),
                            -1,
                            ffi::SQLITE_TRANSIENT(),
                        )
                    }
                }
            };

            if code != ffi::SQLITE_OK {
                return Err(RepoError::Storage(format!("bind failed with code {code}")));
            }
        }
        Ok(())
    }

    fn step(&self) -> c_int {
        // SAFETY: the statement handle is live for the lifetime of `self`.
        unsafe { ffi::sqlite3_step(self.statement) }
    }

    fn column_count(&self) -> c_int {
        // SAFETY: the statement handle is live for the lifetime of `self`.
        unsafe { ffi::sqlite3_column_count(self.statement) }
    }

    fn value(&self, index: c_int) -> Value {
        // SAFETY: the statement handle is live and `index` is within the column
        // range reported by `column_count`.
        unsafe {
            match ffi::sqlite3_column_type(self.statement, index) {
                ffi::SQLITE_NULL => Value::Null,
                ffi::SQLITE_INTEGER => {
                    Value::Integer(ffi::sqlite3_column_int64(self.statement, index))
                }
                ffi::SQLITE_TEXT => {
                    let text = ffi::sqlite3_column_text(self.statement, index);
                    if text.is_null() {
                        return Value::Null;
                    }
                    let length = ffi::sqlite3_column_bytes(self.statement, index);
                    let length = usize::try_from(length).unwrap_or(0);
                    let bytes = std::slice::from_raw_parts(text.cast::<u8>(), length);
                    Value::Text(String::from_utf8_lossy(bytes).into_owned())
                }
                _ => Value::Other,
            }
        }
    }
}

impl SqlSession for Db {
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<usize, RepoError> {
        let statement = self.prepare(sql)?;
        statement.bind(params)?;

        if statement.step() != ffi::SQLITE_DONE {
            return Err(self.error());
        }

        // SAFETY: the handle is live; `sqlite3_changes` only reads connection
        // state.
        let changed = unsafe { ffi::sqlite3_changes(self.handle) };
        Ok(usize::try_from(changed).unwrap_or(0))
    }

    fn execute_batch(&self, sql: &str) -> Result<(), RepoError> {
        let sql = CString::new(sql)
            .map_err(|_| RepoError::Storage("SQL contains a NUL byte".to_owned()))?;

        // SAFETY: the handle is live, `sql` is a valid NUL terminated string,
        // and no callback or error buffer is requested.
        let code = unsafe {
            ffi::sqlite3_exec(
                self.handle,
                sql.as_ptr(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if code != ffi::SQLITE_OK {
            return Err(self.error());
        }
        Ok(())
    }

    fn query_rows(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>, RepoError> {
        let statement = self.prepare(sql)?;
        statement.bind(params)?;

        let columns = statement.column_count();
        let mut rows = Vec::new();

        loop {
            match statement.step() {
                ffi::SQLITE_ROW => {
                    let values = (0..columns).map(|index| statement.value(index)).collect();
                    rows.push(Row::new(values));
                }
                ffi::SQLITE_DONE => return Ok(rows),
                _ => return Err(self.error()),
            }
        }
    }
}
