pub use cratestack_policy::RelationQuantifier;

use super::coalesce::CoalesceFilter;
use super::filter::Filter;
use super::json::JsonFilter;
use super::spatial::SpatialFilter;

#[derive(Debug, Clone, PartialEq)]
pub struct RelationFilter {
    pub quantifier: RelationQuantifier,
    pub parent_table: &'static str,
    pub parent_column: &'static str,
    pub related_table: &'static str,
    pub related_column: &'static str,
    pub filter: Box<FilterExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterExpr {
    Filter(Filter),
    All(Vec<FilterExpr>),
    Any(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
    Relation(RelationFilter),
    /// `COALESCE(col_a, col_b, ...) op value` — built via
    /// [`super::coalesce::coalesce`].
    Coalesce(CoalesceFilter),
    /// JSON / JSONB column predicates — see [`JsonFilter`]. Built via
    /// `FieldRef::json_has_key(...)` and
    /// `FieldRef::json_get_text(...).<cmp>(...)`.
    Json(JsonFilter),
    /// PostGIS spatial predicates — see [`SpatialFilter`]. Built via
    /// `FieldRef::covers_geography(...)` /
    /// `FieldRef::dwithin_geography(...)`. PG-only; the embedded
    /// rusqlite backend doesn't ship SpatiaLite by default, so its
    /// renderer fails loud at codegen time.
    Spatial(SpatialFilter),
}

impl From<Filter> for FilterExpr {
    fn from(value: Filter) -> Self {
        Self::Filter(value)
    }
}

impl RelationFilter {
    pub fn new(
        quantifier: RelationQuantifier,
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        filter: FilterExpr,
    ) -> Self {
        Self {
            quantifier,
            parent_table,
            parent_column,
            related_table,
            related_column,
            filter: Box::new(filter),
        }
    }
}

impl FilterExpr {
    pub fn all(filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        Self::All(filters.into_iter().collect())
    }

    pub fn any(filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        Self::Any(filters.into_iter().collect())
    }

    // A builder-style combinator alongside `all`/`any`; intentionally a
    // by-value method (with double-negation folding), not `ops::Not`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Self {
        match self {
            Self::Not(inner) => *inner,
            inner => Self::Not(Box::new(inner)),
        }
    }

    pub fn and(self, other: impl Into<FilterExpr>) -> Self {
        match (self, other.into()) {
            (Self::All(mut left), Self::All(right)) => {
                left.extend(right);
                Self::All(left)
            }
            (Self::All(mut left), right) => {
                left.push(right);
                Self::All(left)
            }
            (left, Self::All(mut right)) => {
                let mut filters = vec![left];
                filters.append(&mut right);
                Self::All(filters)
            }
            (left, right) => Self::All(vec![left, right]),
        }
    }

    pub fn or(self, other: impl Into<FilterExpr>) -> Self {
        match (self, other.into()) {
            (Self::Any(mut left), Self::Any(right)) => {
                left.extend(right);
                Self::Any(left)
            }
            (Self::Any(mut left), right) => {
                left.push(right);
                Self::Any(left)
            }
            (left, Self::Any(mut right)) => {
                let mut filters = vec![left];
                filters.append(&mut right);
                Self::Any(filters)
            }
            (left, right) => Self::Any(vec![left, right]),
        }
    }

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
