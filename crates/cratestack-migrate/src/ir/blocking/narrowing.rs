//! An enum `CHECK` rebuilt with fewer values than it had
//! (`docs/design/enum-ordinals.md` §3.2). A variant removed from an enum,
//! by name or, for an ordinal enum, with its number moved to `@@reserved`,
//! drops that value from the `CHECK`, and every row still holding it then
//! fails the rebuilt constraint. The diff emits the change as a
//! `DropCheck` and an `AddCheck` of the same name, so the pair is what
//! tells a narrowing from the widening every added variant produces.

use std::collections::BTreeSet;

use crate::ir::{AddCheck, CheckKind, Op};

use super::BlockingReason;

/// The values a narrowed enum `CHECK` no longer accepts, by the table and
/// name of its `AddCheck`. Empty for a widening, a reorder, a rename that
/// keeps an ordinal, and a switch between names and ordinals, whose rows
/// the column's type change rewrites first.
pub(super) fn narrowed_enum_checks(ops: &[Op]) -> Vec<(&AddCheck, Vec<String>)> {
    ops.iter()
        .filter_map(|op| match op {
            Op::AddCheck(add) => Some(add),
            _ => None,
        })
        .filter_map(|add| {
            let dropped = ops.iter().find_map(|op| match op {
                Op::DropCheck(drop) if drop.table == add.table && drop.name == add.name => {
                    Some(&drop.kind)
                }
                _ => None,
            })?;
            if ordinal(dropped) != ordinal(&add.kind) {
                return None;
            }
            let removed: Vec<String> = values(dropped)?
                .difference(&values(&add.kind)?)
                .cloned()
                .collect();
            (!removed.is_empty()).then_some((add, removed))
        })
        .collect()
}

fn ordinal(kind: &CheckKind) -> bool {
    matches!(
        kind,
        CheckKind::Enum {
            ordinals: Some(_),
            ..
        }
    )
}

/// An enum check's accepted values, as the column stores them: its
/// numbers for an ordinal enum, its names otherwise. `None` for any other
/// kind of check.
fn values(kind: &CheckKind) -> Option<BTreeSet<String>> {
    let CheckKind::Enum {
        variants, ordinals, ..
    } = kind
    else {
        return None;
    };
    Some(match ordinals {
        Some(ordinals) => ordinals.iter().map(i64::to_string).collect(),
        None => variants.iter().cloned().collect(),
    })
}

/// Why a narrowed enum `CHECK` blocks, and the backfill that unblocks it.
pub(super) fn reason(check: &AddCheck, removed: &[String]) -> BlockingReason {
    let literals: Vec<String> = if ordinal(&check.kind) {
        removed.to_vec()
    } else {
        removed.iter().map(|name| format!("'{name}'")).collect()
    };
    let list = literals.join(", ");
    BlockingReason {
        table: check.table.clone(),
        column: Some(check.column.clone()),
        cause: format!(
            "enum CHECK `{}` no longer accepts {list}; a row still holding one would violate it",
            check.name
        ),
        remedy: Some(format!(
            "UPDATE {} SET {} = <a value the enum keeps> WHERE {} IN ({list});",
            check.table, check.column, check.column
        )),
    }
}
