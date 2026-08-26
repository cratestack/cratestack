//! Index diff for one (prev, next) table pair.

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
/// `Some`/`Some` goes through [`normalize_predicate`] rather than a
/// byte comparison — see that function's doc for why.
fn predicates_match(prev: Option<&str>, next: Option<&str>) -> bool {
    match (prev, next) {
        (None, None) => true,
        (Some(a), Some(b)) => normalize_predicate(a) == normalize_predicate(b),
        _ => false,
    }
}

/// Best-effort canonicalization used only to decide whether a `where:`
/// predicate changed — never to alter what gets emitted as DDL
/// (`AddIndex::where_predicate` is always carried through and rendered
/// verbatim, see `emit::postgres::indexes`/`emit::sqlite::indexes`).
///
/// Exists because the two sides being compared come from different
/// sources: the `next`-side predicate is the `.cstack` author's literal
/// text, while the `prev`-side predicate (when it came from live
/// introspection rather than a prior schema snapshot) is Postgres's own
/// `pg_get_expr(indpred, indrelid)` deparse of the *stored* predicate,
/// which is always normalized. Verified empirically against a live
/// Postgres 18 (cratestack#742's verification evidence) rather than
/// assumed: every observed predicate, regardless of content, comes back
/// wrapped in exactly one pair of parentheses spanning the whole
/// expression, with internal whitespace collapsed — e.g. the literal
/// `idempotency_key IS NOT NULL` round-trips as
/// `(idempotency_key IS NOT NULL)`. This function reproduces exactly
/// that transformation (whitespace collapse + single outer-paren wrap)
/// so a predicate that survives introspection unchanged compares equal.
///
/// It deliberately does **not** attempt Postgres's other two
/// normalizations — identifier/keyword case-folding and inserting
/// explicit type casts onto literals (`100` → `(100)::numeric`) — since
/// reproducing those correctly needs a real SQL expression parser,
/// which cratestack#742 explicitly scopes out ("Parsing or validating
/// the predicate expression"). A predicate that needs a cast to
/// round-trip byte-for-byte (anything comparing a column to a literal)
/// or that differs from the stored form only by identifier case will
/// still be reported as "changed" and get dropped/recreated on the next
/// plan — a known, documented limitation, not a silent gap: writing the
/// predicate the way Postgres would already deparse it (lowercase
/// identifiers, no comparison literals, or matching whatever the
/// previous introspection already normalized to) avoids the churn.
fn normalize_predicate(predicate: &str) -> String {
    let collapsed: String = predicate.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut inner = collapsed.as_str();
    loop {
        let stripped = strip_matching_outer_parens(inner);
        if stripped == inner {
            break;
        }
        inner = stripped;
    }
    format!("({inner})")
}

/// Strips one pair of parentheses from `value` iff its first `(` and
/// last `)` actually match each other — i.e. the whole string is one
/// parenthesized group, not e.g. `(a) AND (b)`, whose first `(` closes
/// before the string ends. Returns `value` unchanged when it isn't
/// wrapped that way (including when it has no surrounding parens at
/// all), so the caller's loop terminates.
fn strip_matching_outer_parens(value: &str) -> &str {
    if !value.starts_with('(') || !value.ends_with(')') {
        return value;
    }
    let mut depth = 0i32;
    let last_index = value.len() - 1;
    for (index, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && index != last_index {
                    return value;
                }
            }
            _ => {}
        }
    }
    &value[1..last_index]
}

#[cfg(test)]
mod tests {
    use super::normalize_predicate;

    #[test]
    fn wraps_a_bare_predicate_in_one_pair_of_parens() {
        assert_eq!(
            normalize_predicate("idempotency_key IS NOT NULL"),
            "(idempotency_key IS NOT NULL)"
        );
    }

    #[test]
    fn matches_postgres_own_normalized_form() {
        // Observed verbatim from `pg_get_expr` against a live Postgres
        // 18 for `CREATE UNIQUE INDEX ... WHERE idempotency_key IS NOT
        // NULL` (cratestack#742 verification evidence).
        assert_eq!(
            normalize_predicate("idempotency_key IS NOT NULL"),
            normalize_predicate("(idempotency_key IS NOT NULL)")
        );
    }

    #[test]
    fn collapses_irregular_whitespace() {
        assert_eq!(
            normalize_predicate("  idempotency_key   IS  NOT  NULL  "),
            "(idempotency_key IS NOT NULL)"
        );
    }

    #[test]
    fn does_not_merge_two_separately_parenthesized_clauses_into_one() {
        // `(a) AND (b)` must not be mistaken for a single wrapped
        // group — stripping the outer chars naively would produce the
        // syntactically broken `a) AND (b`.
        assert_eq!(
            normalize_predicate("(amount > 100) AND (note = 'x')"),
            "((amount > 100) AND (note = 'x'))"
        );
    }

    #[test]
    fn strips_genuinely_redundant_nested_parens() {
        assert_eq!(
            normalize_predicate("((idempotency_key IS NOT NULL))"),
            "(idempotency_key IS NOT NULL)"
        );
    }

    #[test]
    fn a_changed_predicate_does_not_normalize_equal() {
        assert_ne!(
            normalize_predicate("idempotency_key IS NOT NULL"),
            normalize_predicate("idempotency_key IS NULL")
        );
    }
}
