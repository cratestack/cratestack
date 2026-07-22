//! Column-shape IR nodes: nullability, type, default, plus the
//! `destructiveness_on_add` rule shared by `AddColumn` / `CreateTable`
//! flows.

use serde::{Deserialize, Serialize};

use super::Destructiveness;

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
    /// `@default(dbgenerated())` — a marker, not a value. It asserts
    /// that the column already has (or will separately be given) a
    /// real Postgres-level default set some other way: hand-authored
    /// migration SQL, a trigger, `GENERATED ... AS IDENTITY`, etc.
    /// cratestack has no way to verify that claim from the `.cstack`
    /// schema alone, so emitters must never invent a `DEFAULT` clause
    /// for it — see `Op::destructiveness` and
    /// `crate::ir::unverified_dbgenerated_columns` for the
    /// corresponding safety checks.
    DbGenerated,
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
                // `DbGenerated` is a marker, not a real DDL default —
                // it backfills nothing, so it must not count as "has
                // a default" here any more than no default at all.
                let has_real_default = matches!(
                    self.default,
                    Some(ColumnDefault::Literal(_)) | Some(ColumnDefault::Function(_))
                );
                if has_real_default || self.primary_key {
                    Destructiveness::Safe
                } else {
                    Destructiveness::Blocking
                }
            }
        }
    }
}
