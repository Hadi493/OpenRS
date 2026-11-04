//! Native SQLite driver on `rusqlite`.
//!
//! Durability choices:
//!
//! - WAL keeps readers from blocking the checkout writer and survives crashes.
//! - `synchronous = FULL` trades throughput for "a committed sale is on disk".
//!   The spike measured about 0.15 ms per durable checkout, so this is cheap
//!   enough for retail.
//! - `busy_timeout` is set per connection. Without it a second checkout racing
//!   the first fails with `database is locked` instead of waiting and then
//!   discovering the replay.
//! - `foreign_keys = ON` is per connection; the schema's FKs are not enforced
//!   otherwise.

use std::path::Path;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, params_from_iter};

use super::driver::{Param, Row, SqlSession, Value};
use crate::model::RepoError;

/// One native SQLite connection.
pub(crate) struct Db {
    connection: Connection,
}

impl Db {
    /// Opens an in-memory database.
    pub(crate) fn open_in_memory() -> Result<Self, RepoError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Opens or creates a database file.
    pub(crate) fn open(path: &Path) -> Result<Self, RepoError> {
        Self::from_connection(Connection::open(path)?)
    }

    fn from_connection(connection: Connection) -> Result<Self, RepoError> {
        // `execute_batch` is used because `PRAGMA journal_mode` returns a row,
        // which `execute` rejects.
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(Self { connection })
    }

    /// Runs `f` inside one `IMMEDIATE` transaction.
    ///
    /// `BEGIN IMMEDIATE` takes the write lock up front: two concurrent checkouts
    /// serialize before reading stock instead of failing at commit time. The
    /// rollback on error is explicit so the failure path is visible here rather
    /// than hidden in a drop implementation.
    pub(crate) fn transaction<T>(
        &mut self,
        f: impl FnOnce(&dyn SqlSession) -> Result<T, RepoError>,
    ) -> Result<T, RepoError> {
        self.execute("BEGIN IMMEDIATE", &[])?;

        match f(self) {
            Ok(value) => {
                self.execute("COMMIT", &[])?;
                Ok(value)
            }
            Err(error) => {
                let _ = self.execute("ROLLBACK", &[]);
                Err(error)
            }
        }
    }
}

impl From<rusqlite::Error> for RepoError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

fn sql_params(params: &[Param<'_>]) -> Vec<SqlValue> {
    params
        .iter()
        .map(|param| match param {
            Param::Integer(value) => SqlValue::Integer(*value),
            Param::Text(text) => SqlValue::Text((*text).to_owned()),
        })
        .collect()
}

impl SqlSession for Db {
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<usize, RepoError> {
        Ok(self
            .connection
            .execute(sql, params_from_iter(sql_params(params)))?)
    }

    fn execute_batch(&self, sql: &str) -> Result<(), RepoError> {
        self.connection.execute_batch(sql)?;
        Ok(())
    }

    fn query_rows(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>, RepoError> {
        let mut statement = self.connection.prepare(sql)?;
        let columns = statement.column_count();

        let rows = statement
            .query_map(params_from_iter(sql_params(params)), |row| {
                let mut values = Vec::with_capacity(columns);
                for index in 0..columns {
                    values.push(match row.get_ref(index)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(value) => Value::Integer(value),
                        ValueRef::Text(bytes) => {
                            Value::Text(String::from_utf8_lossy(bytes).into_owned())
                        }
                        ValueRef::Real(_) | ValueRef::Blob(_) => Value::Other,
                    });
                }
                Ok(Row::new(values))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(rows)
    }
}
