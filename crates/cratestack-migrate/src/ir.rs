//! Backend-agnostic migration IR.
//!
//! Every difference the diff engine detects between two schemas is
//! expressed as one or more [`Op`] values. Dialect emitters consume
//! the ops to produce Postgres or SQLite DDL — the IR itself carries
//! no dialect knowledge.
//!
//! Each op has a [`Destructiveness`] class. The generator refuses to
//! emit `Lossy` or `Blocking` ops without explicit opt-in (or, in the
//! `Blocking` case, a sentinel value that proves the operation can be
//! resolved — for example a `NOT NULL` column with a default).

mod checks;
mod columns;
mod foreign_keys;
mod ops;
mod views;

use serde::{Deserialize, Serialize};

pub use checks::{AddCheck, CheckKind, DropCheck};
pub use columns::{Column, ColumnArity, ColumnDefault, ColumnType};
pub use foreign_keys::{AddForeignKey, DropForeignKey};
pub use ops::{
    AddColumn, AddIndex, AlterColumnDefault, AlterColumnNullability, AlterColumnType, CreateTable,
    DropColumn, DropIndex, DropTable, RenameColumn, RenameTable,
};
pub use views::{CreateMaterializedView, CreateView, DropMaterializedView, DropView, ReplaceView};

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
    AddCheck(AddCheck),
    DropCheck(DropCheck),
    AddForeignKey(AddForeignKey),
    DropForeignKey(DropForeignKey),
    CreateView(CreateView),
    DropView(DropView),
    ReplaceView(ReplaceView),
    CreateMaterializedView(CreateMaterializedView),
    DropMaterializedView(DropMaterializedView),
}

impl Op {
    pub fn destructiveness(&self) -> Destructiveness {
        match self {
            Op::CreateTable(_) => Destructiveness::Safe,
            Op::DropTable(_) => Destructiveness::Lossy,
            Op::AddColumn(add) => add.column.destructiveness_on_add(),
            Op::DropColumn(_) => Destructiveness::Lossy,
            Op::AddIndex(_) | Op::DropIndex(_) => Destructiveness::Safe,
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
            // Adding a validator CHECK constraint is conservatively
            // Blocking — existing rows that don't satisfy it will
            // block the ALTER on a non-empty table.
            //
            // An enum CHECK is the exception. It is only ever emitted
            // alongside the column it constrains (CREATE TABLE, ADD
            // COLUMN — no pre-existing rows), or as the second half of
            // a variant *addition*, which widens the accepted set and
            // therefore cannot reject a row that already passed. The
            // one case that can fail on existing data is removing a
            // variant; see the note in `diff::checks`.
            Op::AddCheck(check) => match check.kind {
                CheckKind::Enum { .. } => Destructiveness::Safe,
                _ => Destructiveness::Blocking,
            },
            // Dropping a CHECK constraint never destroys data.
            Op::DropCheck(_) => Destructiveness::Safe,
            // A foreign key can fail to validate against existing
            // orphaned rows, but so can a UNIQUE index against
            // existing duplicates (`Op::AddIndex`, above) — both are
            // classified Safe for the same reason: the DDL either
            // succeeds outright or the migration transaction aborts
            // with no partial data loss. Dropping a foreign key, like
            // dropping any other constraint, never destroys data.
            Op::AddForeignKey(_) | Op::DropForeignKey(_) => Destructiveness::Safe,
            // View creates and replaces never destroy data (the view
            // is a read-only projection over existing tables; replace
            // swaps the SQL body, not the underlying rows).
            Op::CreateView(_) | Op::ReplaceView(_) | Op::CreateMaterializedView(_) => {
                Destructiveness::Safe
            }
            // Dropping a view doesn't destroy source rows but does
            // destroy a queryable surface — treat as Lossy so the
            // generator requires explicit opt-in, mirroring DropTable
            // semantics.
            Op::DropView(_) | Op::DropMaterializedView(_) => Destructiveness::Lossy,
        }
    }
}

/// `(table, column)` pairs, across `CreateTable` and `AddColumn` ops,
/// for `Required` columns whose default is `@default(dbgenerated())`.
///
/// This is a distinct, non-destructive risk from [`Destructiveness`]:
/// the DDL itself always succeeds (no `DEFAULT` clause is emitted —
/// see [`ColumnDefault::DbGenerated`]), but if the column doesn't
/// actually have a real Postgres-level default set some other way,
/// every `INSERT` that omits it will fail with a `NOT NULL` violation
/// at runtime. cratestack cannot verify that from the `.cstack`
/// schema alone, so callers (see `emit::postgres`/`emit::sqlite` and
/// the CLI's `migrate diff`) surface this list as an explicit,
/// non-fatal warning instead.
pub fn unverified_dbgenerated_columns(ops: &[Op]) -> Vec<(String, String)> {
    fn is_unverified(column: &Column) -> bool {
        matches!(column.arity, ColumnArity::Required)
            && matches!(column.default, Some(ColumnDefault::DbGenerated))
    }

    let mut found = Vec::new();
    for op in ops {
        match op {
            Op::CreateTable(create) => {
                for column in &create.columns {
                    if is_unverified(column) {
                        found.push((create.name.clone(), column.name.clone()));
                    }
                }
            }
            Op::AddColumn(add) if is_unverified(&add.column) => {
                found.push((add.table.clone(), add.column.name.clone()));
            }
            _ => {}
        }
    }
    found
}
