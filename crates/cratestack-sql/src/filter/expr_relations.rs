//! Relation-subquery constructors for [`FilterExpr`].
//!
//! Split from `expr.rs` (200-LoC ceiling): that module owns the enum
//! itself plus the boolean combinators (`all`/`any`/`not`/`and`/`or`),
//! this one owns the four `relation*` constructors, which are a
//! distinct concern — they build a correlated-subquery node rather
//! than combining existing predicates.
//!
//! Each constructor takes the related model's [`RelatedReadScope`] as a
//! required argument (GHSA-p55v-6xv5-93p3): pass
//! `<RELATED>_MODEL.related_read_scope()` so the subquery sees only the
//! related rows the caller could read, or [`RelatedReadScope::Unscoped`]
//! only when reading the raw related table is the point.

use cratestack_policy::RelationQuantifier;

use super::expr::{FilterExpr, RelationFilter};
use crate::RelatedReadScope;

impl FilterExpr {
    /// To-one: the related row exists, is visible under `scope`, and
    /// matches `filter`.
    pub fn relation(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
        scope: RelatedReadScope,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::ToOne,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
            scope,
        ))
    }

    /// To-many `some`: at least one related row visible under `scope`
    /// matches `filter`.
    pub fn relation_some(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
        scope: RelatedReadScope,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::Some,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
            scope,
        ))
    }

    /// To-many `every`: no related row visible under `scope` fails
    /// `filter` (vacuously true when none is visible).
    pub fn relation_every(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
        scope: RelatedReadScope,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::Every,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
            scope,
        ))
    }

    /// To-many `none`: no related row visible under `scope` matches
    /// `filter`.
    pub fn relation_none(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
        scope: RelatedReadScope,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::None,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
            scope,
        ))
    }
}
