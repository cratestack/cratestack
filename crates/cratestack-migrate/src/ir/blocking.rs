//! Naming *why* a migration is blocking, not just *that* it is.
//!
//! [`Destructiveness::Blocking`](super::Destructiveness::Blocking) is a
//! single bit, but it is reachable three different ways, each needing a
//! different remedy from the operator. Emitting one generic sentence
//! for all three is how the warning this module exists to support came
//! to describe the wrong operation: it said "a required column was
//! added without a default" even when the actual blocking op was an
//! `Optional → Required` promotion of a column that already existed
//! (cratestack#843).

use std::collections::HashSet;

use super::{ColumnArity, Destructiveness, Op};

/// One blocking operation, described in terms the operator can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingReason {
    /// Table the blocking op targets.
    pub table: String,
    /// Column it targets, or `None` for a table-level constraint.
    pub column: Option<String>,
    /// Why it blocks — one clause, no trailing period.
    pub cause: String,
    /// The shape of the SQL that would unblock it, as a template for
    /// the operator to adapt. `None` when no pre-script can help.
    pub remedy: Option<String>,
}

impl BlockingReason {
    /// `products.version` / `orders` — how this op is addressed in a
    /// comment line.
    pub fn target(&self) -> String {
        match &self.column {
            Some(column) => format!("{}.{}", self.table, column),
            None => self.table.clone(),
        }
    }
}

/// Every blocking op in `ops`, in emission order.
///
/// Empty exactly when no op is blocking in context (see
/// [`destructiveness_in_context`]), so callers can use `!is_empty()` in
/// place of the `has_blocking` bit and get the per-op detail for free.
pub fn blocking_reasons(ops: &[Op]) -> Vec<BlockingReason> {
    let created = tables_created_in(ops);
    ops.iter()
        .filter_map(|op| reason_for(op, &created))
        .collect()
}

/// The destructiveness of each op in `ops`, in order, as the migration
/// as a whole classifies it.
///
/// This is [`Op::destructiveness`] with one context-dependent
/// exemption. A validator CHECK on a table that `ops` itself creates is
/// Safe, because that table has no rows the CHECK could reject
/// (cratestack#1129). `Op` cannot see the rest of the list, so the
/// exemption lives here. Every blocking decision and label must come
/// through this module, not from `Op::destructiveness` directly.
pub fn destructiveness_in_context(ops: &[Op]) -> Vec<Destructiveness> {
    let created = tables_created_in(ops);
    ops.iter().map(|op| class_in(op, &created)).collect()
}

/// Names of the tables `ops` creates. Keyed on the name in the same op
/// list, never on the previous snapshot, so a table that merely
/// exists before the migration is not mistaken for a new one.
fn tables_created_in(ops: &[Op]) -> HashSet<&str> {
    ops.iter()
        .filter_map(|op| match op {
            Op::CreateTable(create) => Some(create.name.as_str()),
            _ => None,
        })
        .collect()
}

fn class_in(op: &Op, created: &HashSet<&str>) -> Destructiveness {
    match op {
        Op::AddCheck(check) if created.contains(check.table.as_str()) => Destructiveness::Safe,
        _ => op.destructiveness(),
    }
}

fn reason_for(op: &Op, created: &HashSet<&str>) -> Option<BlockingReason> {
    match op {
        // A brand-new required column with no default: every existing
        // row needs a value, and no `UPDATE` can supply one before the
        // column exists. The remedy is a two-step add (nullable, then
        // backfill, then promote), which is why there is no one-liner.
        Op::AddColumn(add) if add.column.destructiveness_on_add().is_blocking() => {
            Some(BlockingReason {
                table: add.table.clone(),
                column: Some(add.column.name.clone()),
                cause: "new required column with no default; existing rows have no value for it"
                    .to_owned(),
                remedy: None,
            })
        }
        // The column already exists, so a plain backfill works — this
        // is the case `up.pre.sql` was invented for.
        Op::AlterColumnNullability(alter)
            if matches!(
                (alter.from, alter.to),
                (ColumnArity::Optional, ColumnArity::Required)
            ) =>
        {
            Some(BlockingReason {
                table: alter.table.clone(),
                column: Some(alter.column.clone()),
                cause: "column becomes NOT NULL; existing NULL rows would violate it".to_owned(),
                remedy: Some(format!(
                    "UPDATE {} SET {} = <value> WHERE {} IS NULL;",
                    alter.table, alter.column, alter.column
                )),
            })
        }
        // A CHECK the existing rows may not satisfy. The remedy is
        // row-shape-specific, so we can only gesture at it. Not reached
        // for a table this list creates: `class_in` makes that Safe.
        Op::AddCheck(check) if class_in(op, created).is_blocking() => Some(BlockingReason {
            table: check.table.clone(),
            column: Some(check.column.clone()),
            cause: format!(
                "new CHECK constraint `{}`; existing rows must already satisfy it",
                check.name
            ),
            remedy: Some(format!(
                "UPDATE {} SET {} = <value> WHERE NOT (<the check predicate>);",
                check.table, check.column
            )),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod same_migration_tests;
#[cfg(test)]
mod tests;
