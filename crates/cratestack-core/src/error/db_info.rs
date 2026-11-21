//! [`DbErrorInfo`] — the structured part of a driver-level database error.

use serde::{Deserialize, Serialize};

/// Structured information extracted from a driver-level database error.
///
/// Produced by `cratestack-sqlx`'s [`cratestack_error_from_sqlx`] when the
/// underlying `sqlx::Error` carries a typed `DatabaseError` (e.g.
/// `PgDatabaseError`). Consumers can inspect `constraint`, `table` and
/// `sqlstate` without substring-matching the stringified error message.
///
/// `#[non_exhaustive]`: build one with [`DbErrorInfo::new`] and the
/// `with_*` setters, and read it through the
/// [`CratestackError`](crate::CratestackError) accessors (`db_sqlstate`,
/// `db_constraint`, `db_table`, `db_column`, `db_schema`, `db_message`).
/// Those accessors are the stable, log-safe path: log them rather than
/// `Display` when logs must not carry row values.
///
/// [`cratestack_error_from_sqlx`]: cratestack_sqlx::cratestack_error_from_sqlx
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub struct DbErrorInfo {
    /// The operator-visible detail string: the server's message text.
    pub detail: String,
    /// The five-character SQLSTATE code (`"23505"` for unique_violation, etc.).
    /// `None` when the driver did not surface a code.
    pub sqlstate: Option<String>,
    /// The constraint name reported by the database (`"accounts_email_key"`,
    /// etc.). `None` when the error is not constraint-related.
    pub constraint: Option<String>,
    /// Table, column and schema, boxed and private so the three of them do
    /// not grow `CratestackError` past clippy's `result_large_err`
    /// threshold (128 bytes; it is the `Err` of nearly every fallible
    /// function). Read them with [`Self::table`], [`Self::column`] and
    /// [`Self::schema`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    location: Option<Box<DbErrorLocation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
struct DbErrorLocation {
    table: Option<String>,
    column: Option<String>,
    schema: Option<String>,
}

impl DbErrorInfo {
    /// An info carrying only the operator detail; every other field is `None`.
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            ..Self::default()
        }
    }

    /// Sets the SQLSTATE code.
    #[must_use]
    pub fn with_sqlstate(mut self, sqlstate: impl Into<String>) -> Self {
        self.sqlstate = Some(sqlstate.into());
        self
    }

    /// Sets the constraint name.
    #[must_use]
    pub fn with_constraint(mut self, constraint: impl Into<String>) -> Self {
        self.constraint = Some(constraint.into());
        self
    }

    /// Sets the table name.
    #[must_use]
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.location_mut().table = Some(table.into());
        self
    }

    /// Sets the column name.
    #[must_use]
    pub fn with_column(mut self, column: impl Into<String>) -> Self {
        self.location_mut().column = Some(column.into());
        self
    }

    /// Sets the schema name.
    #[must_use]
    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.location_mut().schema = Some(schema.into());
        self
    }

    /// The table the error was reported against, when the server names one.
    pub fn table(&self) -> Option<&str> {
        self.location.as_deref()?.table.as_deref()
    }

    /// The column the error was reported against (NOT NULL and similar).
    pub fn column(&self) -> Option<&str> {
        self.location.as_deref()?.column.as_deref()
    }

    /// The schema of [`Self::table`], when the server names one.
    pub fn schema(&self) -> Option<&str> {
        self.location.as_deref()?.schema.as_deref()
    }

    fn location_mut(&mut self) -> &mut DbErrorLocation {
        self.location.get_or_insert_with(Default::default)
    }
}
