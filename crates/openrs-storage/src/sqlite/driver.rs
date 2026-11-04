//! Minimal SQL driver surface shared by the native and browser backends.
//!
//! The repository logic is written once against [`SqlSession`]. Each platform
//! implements that trait: `rusqlite` on the desktop, raw SQLite FFI in the
//! browser. Keeping the surface this small is what makes "one transaction
//! model" true in code and not only in the SQL strings.
//!
//! Only integer and text values are supported because the schema only stores
//! those. A REAL or BLOB read back from the database is reported as
//! [`Value::Other`] and rejected by typed accessors, so a schema drift cannot
//! silently truncate a value.

use crate::model::RepoError;

/// A bound SQL parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Param<'a> {
    /// `INTEGER`.
    Integer(i64),
    /// `TEXT`.
    Text(&'a str),
}

/// A column value read from a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    /// SQL `NULL`.
    Null,
    /// SQL `INTEGER`.
    Integer(i64),
    /// SQL `TEXT`.
    Text(String),
    /// A value type the domain never stores (`REAL`, `BLOB`).
    Other,
}

/// One result row, addressed by column position.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    values: Vec<Value>,
}

impl Row {
    pub(crate) fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// Returns the column as an integer.
    pub(crate) fn integer(&self, index: usize) -> Result<i64, RepoError> {
        match self.values.get(index) {
            Some(Value::Integer(value)) => Ok(*value),
            _ => Err(type_error(index, "INTEGER")),
        }
    }

    /// Returns the column as text.
    pub(crate) fn text(&self, index: usize) -> Result<String, RepoError> {
        match self.values.get(index) {
            Some(Value::Text(value)) => Ok(value.clone()),
            _ => Err(type_error(index, "TEXT")),
        }
    }
}

fn type_error(index: usize, expected: &str) -> RepoError {
    RepoError::Storage(format!("column {index} is not {expected}"))
}

/// SQL statements a backend must support.
///
/// Deliberately synchronous: the native driver is synchronous, and the browser
/// driver runs inside a dedicated worker where SQLite is synchronous too. An
/// async adapter belongs above this layer, not inside it.
pub(crate) trait SqlSession {
    /// Runs one statement and returns the number of changed rows.
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<usize, RepoError>;

    /// Runs one or more statements that return no rows, such as migrations and
    /// pragmas.
    fn execute_batch(&self, sql: &str) -> Result<(), RepoError>;

    /// Runs one statement and returns every row it produces.
    fn query_rows(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>, RepoError>;

    /// Runs one statement and returns the first row, if any.
    fn query_row(&self, sql: &str, params: &[Param<'_>]) -> Result<Option<Row>, RepoError> {
        let mut rows = self.query_rows(sql, params)?;
        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(rows.remove(0)))
        }
    }
}
