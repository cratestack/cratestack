//! Relation-subquery constructors for [`FilterExpr`].
//!
//! Split from `expr.rs` (200-LoC ceiling): that module owns the enum
//! itself plus the boolean combinators (`all`/`any`/`not`/`and`/`or`),
//! this one owns the four `relation*` constructors, which are a
//! distinct concern — they build a correlated-subquery node rather
//! than combining existing predicates.

use cratestack_policy::RelationQuantifier;

use super::expr::{FilterExpr, RelationFilter};

impl FilterExpr {
    pub fn relation(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::ToOne,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
        ))
    }

    pub fn relation_some(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::Some,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
        ))
    }

    pub fn relation_every(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::Every,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
        ))
    }

    pub fn relation_none(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
    ) -> Self {
        Self::Relation(RelationFilter::new(
            RelationQuantifier::None,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter,
        ))
    }
}
