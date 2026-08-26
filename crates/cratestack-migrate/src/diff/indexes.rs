//! Index diff for one (prev, next) table pair.
//!
//! The predicate-comparison machinery (`normalize_predicate` and its
//! cast-stripping tokenizer) lives in the sibling `predicate`/
//! `predicate::casts` modules — split out purely to stay under this
//! crate's ~200-LoC-per-file convention; the diff logic in *this* file
//! (`diff_indexes`/`predicates_match`) is the part that actually decides
//! what to drop/recreate.

mod predicate;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::convert::TableProjection;
use crate::ir::{AddIndex, DropIndex, Op};

#[derive(Default)]
pub(super) struct IndexOps {
    pub adds: Vec<Op>,
    pub drops: Vec<Op>,
}

/// Indexes are matched by name — same discipline as every other IR node
/// (`crate::diff`'s module doc). A name collision already implies same
/// table/columns/`using` (`crate::naming::index_name`/`index_name_unique`
/// fold every one of those into the name), so the one thing that can
/// differ under an unchanged name is the `where:` partial-index
/// predicate (issue #742) — checked explicitly here and, if changed,
/// treated as a drop + recreate, since neither Postgres nor SQLite
/// supports an in-place `ALTER INDEX ... WHERE`.
pub(super) fn diff_indexes(prev: &TableProjection, next: &TableProjection) -> IndexOps {
    let mut out = IndexOps::default();

    let prev_by_name: BTreeMap<&str, &AddIndex> =
        prev.indexes.iter().map(|i| (i.name.as_str(), i)).collect();
    let next_by_name: BTreeMap<&str, &AddIndex> =
        next.indexes.iter().map(|i| (i.name.as_str(), i)).collect();

    for index in &prev.indexes {
        match next_by_name.get(index.name.as_str()) {
            None => out.drops.push(Op::DropIndex(DropIndex {
                name: index.name.clone(),
                table: index.table.clone(),
            })),
            Some(next_index) => {
                if !predicates_match(
                    index.where_predicate.as_deref(),
                    next_index.where_predicate.as_deref(),
                ) {
                    out.drops.push(Op::DropIndex(DropIndex {
                        name: index.name.clone(),
                        table: index.table.clone(),
                    }));
                    out.adds.push(Op::AddIndex((*next_index).clone()));
                }
            }
        }
    }
    for index in &next.indexes {
        if !prev_by_name.contains_key(index.name.as_str()) {
            out.adds.push(Op::AddIndex(index.clone()));
        }
    }

    out
}

/// Whether two `where:` predicates should be treated as the same
/// constraint. `None` on both sides is the common (non-partial) case.
/// `Some`/`Some` goes through [`predicate::normalize_predicate`] rather
/// than a byte comparison — see that function's doc for why.
fn predicates_match(prev: Option<&str>, next: Option<&str>) -> bool {
    match (prev, next) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            predicate::normalize_predicate(a) == predicate::normalize_predicate(b)
        }
        _ => false,
    }
}
