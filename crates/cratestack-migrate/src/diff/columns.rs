//! Column-level diff for one (prev, next) table pair.

use std::collections::{BTreeMap, BTreeSet};

use crate::convert::TableProjection;
use crate::ir::{
    AddColumn, AlterColumnDefault, AlterColumnNullability, AlterColumnType, Column, ColumnArity,
    DropColumn, Op, RenameColumn,
};

/// Per-table column-diff result. Each vector is appended to the
/// migration-wide bucket of the same name by the caller.
#[derive(Default)]
pub(super) struct ColumnOps {
    pub renames: Vec<Op>,
    pub drops: Vec<Op>,
    pub adds: Vec<Op>,
    pub alters: Vec<Op>,
}

pub(super) fn diff_columns(prev: &TableProjection, next: &TableProjection) -> ColumnOps {
    let mut out = ColumnOps::default();

    let prev_columns: BTreeMap<_, _> = prev
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect();
    let next_columns: BTreeMap<_, _> = next
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect();

    // The table name to put on emitted ops is the *new* name —
    // RenameTable lands earlier in the migration, so all subsequent
    // column ops should reference the post-rename table name.
    let effective_table = next.name.as_str();

    // Column-level renames: a next-side column with
    // `@rename(from = "...")` consumes the matching prev-side column
    // so we don't emit drop+add for it.
    let column_renamed_from: BTreeMap<&str, &str> = next
        .column_renames
        .iter()
        .filter_map(|(new, old)| {
            if prev_columns.contains_key(old.as_str())
                && !prev_columns.contains_key(new.as_str())
                && next_columns.contains_key(new.as_str())
            {
                Some((new.as_str(), old.as_str()))
            } else {
                None
            }
        })
        .collect();
    let consumed_prev_columns: BTreeSet<&str> = column_renamed_from.values().copied().collect();

    for (new_name, old_name) in &column_renamed_from {
        out.renames.push(Op::RenameColumn(RenameColumn {
            table: effective_table.to_owned(),
            from: (*old_name).to_owned(),
            to: (*new_name).to_owned(),
        }));
    }

    for (column_name, _) in &prev_columns {
        if consumed_prev_columns.contains(column_name) {
            continue;
        }
        if !next_columns.contains_key(column_name) {
            out.drops.push(Op::DropColumn(DropColumn {
                table: effective_table.to_owned(),
                column: (*column_name).to_owned(),
            }));
        }
    }

    for (column_name, column) in &next_columns {
        if column_renamed_from.contains_key(column_name) {
            continue;
        }
        if !prev_columns.contains_key(column_name) {
            out.adds.push(Op::AddColumn(AddColumn {
                table: effective_table.to_owned(),
                column: (*column).clone(),
            }));
        }
    }

    // Columns present in both — emit alter ops for shape changes.
    // Includes columns that were renamed: their identity (the value
    // the user means) is preserved, so a type or default change on a
    // renamed column still produces an alter op.
    for (column_name, prev_column) in &prev_columns {
        // If this prev-column was consumed by a rename, compare
        // against the *new* column on the next side.
        let renamed_to = column_renamed_from
            .iter()
            .find_map(|(new, old)| (*old == *column_name).then_some(*new));
        let next_column = match renamed_to {
            Some(new_name) => next_columns.get(new_name),
            None => next_columns.get(column_name),
        };
        let Some(next_column) = next_column else {
            continue;
        };
        // For alter ops, use the *new* column name so they line up
        // with the rename emitted earlier.
        let effective_column = match renamed_to {
            Some(new_name) => new_name,
            None => *column_name,
        };
        let mut with_effective_name = (*prev_column).clone();
        with_effective_name.name = effective_column.to_owned();
        out.alters.extend(column_alter_ops(
            effective_table,
            &with_effective_name,
            next_column,
        ));
    }

    out
}

/// Compare a column's previous and next definitions and emit the
/// alter ops required to bring `prev` into `next` shape. Order
/// inside the returned vector is intentional: type changes before
/// nullability before defaults, so each subsequent op can rely on
/// the previous one having landed.
fn column_alter_ops(table: &str, prev: &Column, next: &Column) -> Vec<Op> {
    let mut ops = Vec::new();

    if prev.ty != next.ty || prev.arity != next.arity {
        // Only emit AlterColumnType when the *type itself* or the
        // list-vs-scalar shape changes. A pure Required ↔ Optional
        // flip is handled by AlterColumnNullability below.
        let type_changed = prev.ty != next.ty;
        let shape_changed =
            matches!(prev.arity, ColumnArity::List) != matches!(next.arity, ColumnArity::List);
        if type_changed || shape_changed {
            ops.push(Op::AlterColumnType(AlterColumnType {
                table: table.to_owned(),
                column: prev.name.clone(),
                from: prev.ty.clone(),
                from_arity: prev.arity,
                to: next.ty.clone(),
                to_arity: next.arity,
            }));
        }
    }

    if prev.arity != next.arity {
        ops.push(Op::AlterColumnNullability(AlterColumnNullability {
            table: table.to_owned(),
            column: prev.name.clone(),
            from: prev.arity,
            to: next.arity,
        }));
    }

    if prev.default != next.default {
        ops.push(Op::AlterColumnDefault(AlterColumnDefault {
            table: table.to_owned(),
            column: prev.name.clone(),
            from: prev.default.clone(),
            to: next.default.clone(),
        }));
    }

    ops
}
