//! CHECK-constraint diff for one (prev, next) table pair.

use std::collections::{BTreeMap, BTreeSet};

use crate::convert::TableProjection;
use crate::ir::{DropCheck, Op};

#[derive(Default)]
pub(super) struct CheckOps {
    pub adds: Vec<Op>,
    pub drops: Vec<Op>,
}

pub(super) fn diff_checks(prev: &TableProjection, next: &TableProjection) -> CheckOps {
    let mut out = CheckOps::default();

    let prev_check_names: BTreeSet<&str> = prev.checks.iter().map(|c| c.name.as_str()).collect();
    let next_checks_by_name: BTreeMap<&str, &crate::ir::AddCheck> =
        next.checks.iter().map(|c| (c.name.as_str(), c)).collect();

    for check in &prev.checks {
        if !next_checks_by_name.contains_key(check.name.as_str()) {
            out.drops.push(Op::DropCheck(DropCheck {
                table: check.table.clone(),
                column: check.column.clone(),
                name: check.name.clone(),
                kind: check.kind.clone(),
            }));
        }
    }
    for (name, check) in &next_checks_by_name {
        if !prev_check_names.contains(name) {
            out.adds.push(Op::AddCheck((*check).clone()));
        } else if let Some(prev_check) = prev.checks.iter().find(|c| c.name.as_str() == *name) {
            // Same name on both sides — compare kinds. A kind change
            // means the bounds tightened/loosened, the validator type
            // changed, or (for `CheckKind::Enum`) the variant set
            // changed. Emit drop + add.
            //
            // Adding an enum variant widens the accepted set, so the
            // rebuilt constraint cannot reject a row that already
            // passed — this is the path that replaces the old
            // `ALTER TYPE ... ADD VALUE`, without its transactional
            // restrictions. *Removing* a variant narrows the set and
            // can fail on existing rows holding the removed value;
            // that is a genuine schema-level decision the diff engine
            // does not try to backfill around.
            if prev_check.kind != check.kind {
                out.drops.push(Op::DropCheck(DropCheck {
                    table: prev_check.table.clone(),
                    column: prev_check.column.clone(),
                    name: prev_check.name.clone(),
                    kind: prev_check.kind.clone(),
                }));
                out.adds.push(Op::AddCheck((*check).clone()));
            }
        }
    }

    out
}
