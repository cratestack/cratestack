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

use super::{ColumnArity, Op};

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
/// Empty exactly when no op is
/// [`Destructiveness::Blocking`](super::Destructiveness::Blocking), so
/// callers can use `!is_empty()` in place of the `has_blocking` bit and
/// get the per-op detail for free.
pub fn blocking_reasons(ops: &[Op]) -> Vec<BlockingReason> {
    ops.iter().filter_map(reason_for).collect()
}

fn reason_for(op: &Op) -> Option<BlockingReason> {
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
        // row-shape-specific, so we can only gesture at it.
        Op::AddCheck(check) if op.destructiveness().is_blocking() => Some(BlockingReason {
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
mod tests;
