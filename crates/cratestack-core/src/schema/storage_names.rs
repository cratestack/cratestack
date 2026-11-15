//! SQL storage names, `@@map("<table>")` on a model and `@map("<column>")` on
//! a stored field (cratestack#1375).
//!
//! The one reader for every consumer: `cratestack-parser` validates a marker
//! with [`parse_map_name`] and [`validate_sql_identifier`]; `cratestack-macros`,
//! `cratestack-migrate` and `cratestack-studio` ask [`Model::sql_table_name`] /
//! [`Field::sql_column_name`] for the name SQL uses. The mapping is
//! storage-only: REST paths, RPC op ids, client accessors, generated
//! identifiers and wire field names all come from the model and field names,
//! and never from here.

use super::model::{Field, Model};
use super::sql_reserved_words::is_sql_reserved_word;
use crate::route_naming::{pluralize, to_snake_case};

/// The one argument shape a map marker takes, for error messages.
pub const MAP_ARGUMENT_FORM: &str = "\"<sql_name>\"";

/// The longest a PostgreSQL identifier may be, in bytes.
pub const SQL_IDENTIFIER_MAX_BYTES: usize = 63;

/// Prefixes a mapped table may not start with: cratestack's own tables, and
/// the two dialects' catalogs.
pub const RESERVED_TABLE_PREFIXES: &[&str] = &["cratestack_", "sqlite_", "pg_"];

/// What a mapped name names, for [`validate_sql_identifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageNameKind {
    Table,
    Column,
}

impl StorageNameKind {
    fn word(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Column => "column",
        }
    }
}

/// The name in `arguments`, the text inside a map marker's parentheses: one
/// double-quoted, non-empty name with no `"` or `\` in it, whitespace allowed
/// around it. `None` for anything else.
pub fn parse_map_name(arguments: &str) -> Option<&str> {
    let name = arguments.trim().strip_prefix('"')?.strip_suffix('"')?;
    let plain = !name.is_empty() && !name.contains(['"', '\\']);
    plain.then_some(name)
}

/// The name in a whole map marker `raw`: `marker` (`"@map"` or `"@@map"`),
/// directly its `(`, arguments [`parse_map_name`] takes, and a `)` that ends
/// the text. `None` for anything else.
pub fn map_marker_name<'a>(raw: &'a str, marker: &str) -> Option<&'a str> {
    let arguments = raw
        .strip_prefix(marker)?
        .strip_prefix('(')?
        .strip_suffix(')')?;
    parse_map_name(arguments)
}

/// Why `name` cannot be a mapped `kind`, or `Ok`. The grammar is
/// `^[a-z_][a-z0-9_]*$`, at most [`SQL_IDENTIFIER_MAX_BYTES`] bytes, not a
/// reserved word, and a table not starting with a [`RESERVED_TABLE_PREFIXES`]
/// entry: no generated SQL quotes a name, so anything else would not mean the
/// same thing to every statement (and a `.` would name another schema).
pub fn validate_sql_identifier(name: &str, kind: StorageNameKind) -> Result<(), String> {
    let word = kind.word();
    let mut chars = name.chars();
    let starts_well = chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_lowercase());
    if !starts_well || !chars.all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit()) {
        return Err(format!(
            "`{name}` is not a valid {word} name: use lower-case letters, digits and `_`, \
             starting with a letter or `_` (no `.`, no quoting)"
        ));
    }
    if name.len() > SQL_IDENTIFIER_MAX_BYTES {
        return Err(format!(
            "`{name}` is {} bytes; a {word} name is at most {SQL_IDENTIFIER_MAX_BYTES}",
            name.len()
        ));
    }
    if is_sql_reserved_word(name) {
        return Err(format!(
            "`{name}` is a reserved SQL word and generated SQL does not quote names; \
             choose another {word} name"
        ));
    }
    if kind == StorageNameKind::Table
        && let Some(prefix) = RESERVED_TABLE_PREFIXES
            .iter()
            .find(|prefix| name.starts_with(**prefix))
    {
        return Err(format!(
            "`{name}` starts with `{prefix}`, which is reserved for cratestack and the database"
        ));
    }
    Ok(())
}

impl Model {
    /// The table this model is stored in: its `@@map("...")` name, else
    /// `pluralize(to_snake_case(name))`. Never a route: the REST path is
    /// [`crate::route_naming::model_route_segment`].
    pub fn sql_table_name(&self) -> String {
        self.attributes
            .iter()
            .find_map(|attribute| map_marker_name(&attribute.raw, "@@map"))
            .map_or_else(|| pluralize(&to_snake_case(&self.name)), str::to_owned)
    }

    /// The column of this model's field called `field`, or `None` when the
    /// model has no such field.
    pub fn sql_column_of(&self, field: &str) -> Option<String> {
        self.fields
            .iter()
            .find(|candidate| candidate.name == field)
            .map(Field::sql_column_name)
    }
}

impl Field {
    /// The column this field is stored in: its `@map("...")` name, else
    /// `to_snake_case(name)`. Never a wire name: JSON/CBOR keys are the field
    /// name as written.
    pub fn sql_column_name(&self) -> String {
        self.attributes
            .iter()
            .find_map(|attribute| map_marker_name(&attribute.raw, "@map"))
            .map_or_else(|| to_snake_case(&self.name), str::to_owned)
    }
}

#[cfg(test)]
#[path = "storage_names_tests.rs"]
mod tests;
