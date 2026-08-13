//! Runtime representation of a traversed relation path.
//!
//! Generated relation accessors (`post::author().profile().nickname()`)
//! accumulate a `RelationHop` per traversed relation and fold them into a
//! `FilterExpr` or an `OrderClause` at call time.
//!
//! This is deliberately a *runtime* value rather than a compile-time type
//! chain. Encoding the path in the module/type tree — one `Path` type per
//! distinct path — makes the emitted code exponential in relation-graph
//! connectivity, because the number of simple paths through a graph is
//! exponential in its connectivity (cratestack#252: 6 chained models cost
//! 9.5 min / 10.5 GB to expand; a 16-model schema could not build at all).
//! Carrying the path as data makes codegen linear in `models × fields`:
//! each model emits exactly one `Path`, and the fold below replaces the
//! per-path token duplication.
//!
//! Every hop's table/column names are `&'static str` baked in by the macro,
//! so filter folding allocates nothing; only order rendering builds a
//! `String`, because the correlated-subquery chain is genuinely
//! path-dependent.

use crate::filter::{FilterExpr, RelationQuantifier};

/// Marker for a path whose hops are all to-one, so a scalar at the end of
/// it can be rendered as a correlated subquery and used for ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orderable;

/// Marker for a path that has crossed a to-many hop. Ordering accessors are
/// not implemented for this marker, which reproduces the old guarantee that
/// `asc()`/`desc()` simply did not exist past a to-many relation — a
/// compile error, not a runtime failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unorderable;

/// One traversed relation edge: the FK linkage plus how the related rows
/// are quantified (`ToOne` for a plain to-one hop, `Some`/`Every`/`None`
/// for a to-many hop under a quantifier).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationHop {
    pub parent_table: &'static str,
    pub parent_column: &'static str,
    pub related_table: &'static str,
    pub related_column: &'static str,
    pub quantifier: RelationQuantifier,
}

impl RelationHop {
    pub const fn new(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        quantifier: RelationQuantifier,
    ) -> Self {
        Self {
            parent_table,
            parent_column,
            related_table,
            related_column,
            quantifier,
        }
    }

    /// Same linkage, re-quantified. Used when a to-many hop is recorded
    /// before the caller has picked `some`/`every`/`none`.
    pub const fn with_quantifier(self, quantifier: RelationQuantifier) -> Self {
        Self { quantifier, ..self }
    }
}

/// Fold a scalar `FilterExpr` outward through the traversed path, applying
/// each hop's quantifier. Mirrors what the macro previously emitted as
/// nested `FilterExpr::relation*(...)` token trees.
pub fn wrap_filter(hops: &[RelationHop], inner: FilterExpr) -> FilterExpr {
    hops.iter()
        .rev()
        .fold(inner, |acc, hop| match hop.quantifier {
            RelationQuantifier::ToOne => FilterExpr::relation(
                hop.parent_table,
                hop.parent_column,
                hop.related_table,
                hop.related_column,
                acc,
            ),
            RelationQuantifier::Some => FilterExpr::relation_some(
                hop.parent_table,
                hop.parent_column,
                hop.related_table,
                hop.related_column,
                acc,
            ),
            RelationQuantifier::Every => FilterExpr::relation_every(
                hop.parent_table,
                hop.parent_column,
                hop.related_table,
                hop.related_column,
                acc,
            ),
            RelationQuantifier::None => FilterExpr::relation_none(
                hop.parent_table,
                hop.parent_column,
                hop.related_table,
                hop.related_column,
                acc,
            ),
        })
}

/// Build the correlated-subquery expression that yields `column` at the end
/// of `hops`, relative to the table reached by the *first* hop.
///
/// `hops[0]` is carried on the `OrderClause` itself (it becomes the clause's
/// parent/related linkage), so only `hops[1..]` are nested here — matching
/// the shape the macro used to compute at expansion time.
///
/// Panics if `hops` is empty; callers only reach this from a generated
/// accessor that has traversed at least one relation.
pub fn order_value_sql(hops: &[RelationHop], column: &str) -> String {
    assert!(
        !hops.is_empty(),
        "order_value_sql requires at least one relation hop",
    );
    let mut sql = format!("{}.{}", hops[hops.len() - 1].related_table, column,);
    for index in (1..hops.len()).rev() {
        let hop = &hops[index];
        let current_table = hops[index - 1].related_table;
        sql = format!(
            "(SELECT {} FROM {} WHERE {}.{} = {}.{} LIMIT 1)",
            sql,
            hop.related_table,
            hop.related_table,
            hop.related_column,
            current_table,
            hop.parent_column,
        );
    }
    sql
}

/// Whether every hop is to-one. Ordering through a to-many hop is not
/// expressible as a scalar correlated subquery, so generated `asc()`/
/// `desc()` accessors are gated on this (previously enforced by simply not
/// emitting those methods past a to-many hop).
pub fn is_orderable(hops: &[RelationHop]) -> bool {
    hops.iter()
        .all(|hop| matches!(hop.quantifier, RelationQuantifier::ToOne))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn to_one(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
    ) -> RelationHop {
        RelationHop::new(
            parent_table,
            parent_column,
            related_table,
            related_column,
            RelationQuantifier::ToOne,
        )
    }

    #[test]
    fn single_hop_reads_the_related_table_directly() {
        let hops = [to_one("posts", "author_id", "users", "id")];
        assert_eq!(order_value_sql(&hops, "email"), "users.email");
    }

    #[test]
    fn two_hops_nest_a_correlated_subquery() {
        let hops = [
            to_one("posts", "author_id", "users", "id"),
            to_one("users", "profile_id", "profiles", "id"),
        ];
        assert_eq!(
            order_value_sql(&hops, "nickname"),
            "(SELECT profiles.nickname FROM profiles \
             WHERE profiles.id = users.profile_id LIMIT 1)",
        );
    }

    #[test]
    fn a_to_many_hop_makes_the_path_unorderable() {
        let hops = [
            to_one("posts", "author_id", "users", "id"),
            RelationHop::new(
                "users",
                "id",
                "comments",
                "user_id",
                RelationQuantifier::Some,
            ),
        ];
        assert!(!is_orderable(&hops));
        assert!(is_orderable(&hops[..1]));
    }
}
