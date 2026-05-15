//! Wire types for validation failures.
//!
//! Kept separate from the entry module so the API layer can pull just
//! the types without depending on the validation engine.

use serde::Serialize;

/// Per-field validation failure. The wire shape is stable: code +
/// human-readable message + the field name that failed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub code: ValidationCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationCode {
    /// `null` (or missing key) on a required, non-defaulted field.
    Required,
    /// Value is not a JSON string / number / boolean as expected.
    TypeMismatch,
    /// `@email` rejected the value.
    Email,
    /// `@length(min: …, max: …)` rejected the value.
    Length,
    /// `@range(min: …, max: …)` rejected the value.
    Range,
    /// `@regex("…")` rejected the value.
    Regex,
    /// `@uri` rejected the value.
    Uri,
    /// `@iso4217` rejected the value (currency code).
    Iso4217,
    /// A unique constraint rejected the value at the database layer
    /// (Postgres SQLSTATE `23505`, SQLite `SQLITE_CONSTRAINT_UNIQUE` /
    /// `SQLITE_CONSTRAINT_PRIMARYKEY`).
    Unique,
    /// A foreign-key constraint rejected the value at the database
    /// layer (Postgres `23503`, SQLite `SQLITE_CONSTRAINT_FOREIGNKEY`).
    ForeignKey,
}
