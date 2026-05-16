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
mod enums;
mod ops;

use serde::{Deserialize, Serialize};

pub use checks::{AddCheck, CheckKind, DropCheck};
pub use columns::{Column, ColumnArity, ColumnDefault, ColumnType};
pub use enums::{AlterEnumAddVariant, CreateEnum, DropEnum};
pub use ops::{
    AddColumn, AddIndex, AlterColumnDefault, AlterColumnNullability, AlterColumnType, CreateTable,
    DropColumn, DropIndex, DropTable, RenameColumn, RenameTable,
};

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
    CreateEnum(CreateEnum),
    AlterEnumAddVariant(AlterEnumAddVariant),
    DropEnum(DropEnum),
    AddCheck(AddCheck),
    DropCheck(DropCheck),
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
            // Creating an enum or adding a variant is safe. Dropping
            // an enum entirely is lossy (rows that reference it on
            // other tables would need to be migrated first; the
            // generator does not attempt that automatically).
            Op::CreateEnum(_) | Op::AlterEnumAddVariant(_) => Destructiveness::Safe,
            Op::DropEnum(_) => Destructiveness::Lossy,
            // Adding a CHECK constraint is conservatively Blocking —
            // existing rows that don't satisfy it will block the
            // ALTER on a non-empty table.
            Op::AddCheck(_) => Destructiveness::Blocking,
            // Dropping a CHECK constraint never destroys data.
            Op::DropCheck(_) => Destructiveness::Safe,
        }
    }
}
