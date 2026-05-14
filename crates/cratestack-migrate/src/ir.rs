//! Backend-agnostic migration IR.
//!
//! Every difference the diff engine detects between two schemas is
//! expressed as one or more [`Op`] values. Dialect emitters consume the
//! ops to produce Postgres or SQLite DDL — the IR itself carries no
//! dialect knowledge.
//!
//! Each op has a [`Destructiveness`] class. The generator refuses to
//! emit `Lossy` or `Blocking` ops without explicit opt-in (or, in the
//! `Blocking` case, a sentinel value that proves the operation can be
//! resolved — for example a `NOT NULL` column with a default).

use serde::{Deserialize, Serialize};

/// How dangerous an operation is to apply.
///
/// * `Safe` — never destroys data, never blocks on existing data.
/// * `Lossy` — destroys data (`DROP COLUMN`, `DROP TABLE`, narrowing).
/// * `Blocking` — cannot succeed without resolving a precondition
///   (adding `NOT NULL` to a non-empty table without a default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Destructiveness {
    Safe,
    Lossy,
    Blocking,
}

/// Column data shared by `CreateTable` and `AddColumn`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub arity: ColumnArity,
    pub default: Option<ColumnDefault>,
    pub primary_key: bool,
}

/// Column nullability and shape.
///
/// `List` corresponds to a `.cstack` list field (`String[]`). The
/// Postgres emitter renders it as a SQL array; the SQLite emitter
/// rejects it at emit time (SQLite has no array type and the right
/// answer is a relation table or a JSON column, both of which require
/// schema-level decisions the diff engine cannot make).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnArity {
    Required,
    Optional,
    List,
}

/// Column type. The diff engine keeps the `.cstack` scalar name as a
/// string and defers dialect mapping to the emitter — this way new
/// scalars do not require touching the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    /// `.cstack` built-in scalar (`String`, `Int`, `Uuid`, …).
    Scalar(String),
    /// User-defined enum declared via `enum Name { … }`.
    Enum(String),
    /// User-defined composite type declared via `type Name { … }`.
    UserDefined(String),
}

/// Column default value, captured as the developer wrote it. The
/// emitter is responsible for any dialect-specific quoting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnDefault {
    /// Literal (e.g. `0`, `'pending'`, `true`).
    Literal(String),
    /// Database function (e.g. `now()`, `gen_random_uuid()`).
    Function(String),
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterColumnDefault {
    pub table: String,
    pub column: String,
    pub from: Option<ColumnDefault>,
    pub to: Option<ColumnDefault>,
}

/// One migration operation. See [module docs](self) for context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    CreateTable(CreateTable),
    DropTable(DropTable),
    AddColumn(AddColumn),
    DropColumn(DropColumn),
    AddIndex(AddIndex),
    DropIndex(DropIndex),
    AlterColumnType(AlterColumnType),
    AlterColumnNullability(AlterColumnNullability),
    AlterColumnDefault(AlterColumnDefault),
    RenameTable(RenameTable),
    RenameColumn(RenameColumn),
}

impl Op {
    pub fn destructiveness(&self) -> Destructiveness {
        match self {
            Op::CreateTable(_) => Destructiveness::Safe,
            Op::DropTable(_) => Destructiveness::Lossy,
            Op::AddColumn(add) => add.column.destructiveness_on_add(),
            Op::DropColumn(_) => Destructiveness::Lossy,
            Op::AddIndex(_) => Destructiveness::Safe,
            Op::DropIndex(_) => Destructiveness::Safe,
            // Type changes are conservatively Lossy. The IR has no
            // dialect-aware view on widening vs narrowing — Postgres
            // will reject a narrowing cast at runtime, but the diff
            // engine must not silently emit one as Safe.
            Op::AlterColumnType(_) => Destructiveness::Lossy,
            Op::AlterColumnNullability(alter) => match (alter.from, alter.to) {
                // Loosening (Required → Optional) is always Safe.
                (ColumnArity::Required, ColumnArity::Optional) => Destructiveness::Safe,
                // Tightening (Optional → Required) cannot succeed on
                // existing NULL rows — Blocking until backfilled.
                (ColumnArity::Optional, ColumnArity::Required) => Destructiveness::Blocking,
                // List ↔ scalar arity flips reshape data — Lossy.
                _ => Destructiveness::Lossy,
            },
            // Default-value changes don't touch existing rows.
            Op::AlterColumnDefault(_) => Destructiveness::Safe,
            // Renames preserve all data; both backends support
            // ALTER TABLE … RENAME on modern versions.
            Op::RenameTable(_) | Op::RenameColumn(_) => Destructiveness::Safe,
        }
    }
}

impl Column {
    /// Whether adding this column to an existing table is safe,
    /// blocking, or otherwise.
    ///
    /// * Optional columns are always safe — every existing row gets
    ///   `NULL` for the new column.
    /// * Required columns with a default are safe — Postgres and
    ///   SQLite both backfill the default into every existing row.
    /// * Required columns without a default are **blocking** — the
    ///   migration cannot succeed on a non-empty table; the user must
    ///   either set a default in the schema or supply backfill SQL in
    ///   `up.pre.sql`.
    pub(crate) fn destructiveness_on_add(&self) -> Destructiveness {
        match self.arity {
            ColumnArity::Optional | ColumnArity::List => Destructiveness::Safe,
            ColumnArity::Required => {
                if self.default.is_some() || self.primary_key {
                    Destructiveness::Safe
                } else {
                    Destructiveness::Blocking
                }
            }
        }
    }
}
