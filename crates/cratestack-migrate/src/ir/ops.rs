//! Op-payload structs for table/column/index/rename operations.
//! Check-constraint ops live in [`super::checks`] — including the
//! enum membership constraint that stands in for a native enum type.

use serde::{Deserialize, Serialize};

use super::columns::{Column, ColumnArity, ColumnDefault, ColumnType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropTable {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddColumn {
    pub table: String,
    pub column: Column,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropColumn {
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddIndex {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// Index access method (Postgres's `USING <method>` clause), e.g.
    /// `ivfflat`/`hnsw` for a pgvector approximate-nearest-neighbor
    /// index — see `docs/design/extensions.md` §6 and issue #156.
    /// `None` renders the exact same plain `CREATE [UNIQUE] INDEX ...
    /// (columns)` DDL this crate always emitted, so every pre-existing
    /// `@unique`/`@@unique([...])`-derived index is unaffected by this
    /// field's addition — Postgres's own default access method
    /// (`btree`) is left implicit rather than spelled out.
    #[serde(default)]
    pub using: Option<String>,
    /// Operator class applied to every column in `columns` (e.g.
    /// `vector_l2_ops`). Only meaningful alongside `using`; `None`
    /// leaves each column's default operator class in place.
    #[serde(default)]
    pub opclass: Option<String>,
    /// `WHERE <predicate>` clause (cratestack#742) — makes this a
    /// *partial* index, present only for rows matching the predicate.
    /// Carried verbatim from the `.cstack` `where: "..."` keyword
    /// argument (schema-side) or from Postgres's own
    /// `pg_get_expr(indpred, indrelid)` (introspection-side, always
    /// normalized text — see `crate::introspect::postgres::indexes` and
    /// `crate::diff::indexes` for how the diff engine tolerates that).
    /// `None` renders the exact same plain `CREATE [UNIQUE] INDEX ...
    /// (columns)` DDL this crate always emitted, so every pre-existing
    /// `@unique`/`@@unique([...])`-derived index is unaffected by this
    /// field's addition.
    #[serde(default)]
    pub where_predicate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropIndex {
    pub name: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterColumnType {
    pub table: String,
    pub column: String,
    pub from: ColumnType,
    pub from_arity: ColumnArity,
    pub to: ColumnType,
    pub to_arity: ColumnArity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterColumnNullability {
    pub table: String,
    pub column: String,
    pub from: ColumnArity,
    pub to: ColumnArity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterColumnDefault {
    pub table: String,
    pub column: String,
    pub from: Option<ColumnDefault>,
    pub to: Option<ColumnDefault>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameTable {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameColumn {
    pub table: String,
    pub from: String,
    pub to: String,
}
