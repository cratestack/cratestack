use crate::order::{OrderClause, OrderTarget};
use crate::values::{FilterValue, IntoSqlValue};
use crate::{NullOrder, SortDirection};

use super::expr::FilterExpr;
use super::op::FilterOp;

/// Distance metric for a `Vector(n)` similarity search (see
/// `docs/design/extensions.md` §6/§7, cratestack#163). Maps 1:1 onto
/// pgvector's three distance operators and the `opclass` names used by
/// `@@index([...], opclass: "...")` (cratestack#156's DDL) — but is
/// never *inferred* from an index: an index is only ever an optional
/// access-path speedup, and AC #2 on cratestack#163 requires distance
/// ordering/filtering to keep working with no vector index present at
/// all (a plain sequential scan), so callers state the metric
/// explicitly at the call site. [`VectorMetric::from_opclass`] is a
/// convenience for callers that already know their index's opclass and
/// don't want to duplicate the mapping by hand — it is never called
/// automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorMetric {
    /// Euclidean (L2) distance — operator `<->`, opclass `vector_l2_ops`.
    L2,
    /// Cosine distance — operator `<=>`, opclass `vector_cosine_ops`.
    Cosine,
    /// Negative inner product — operator `<#>`, opclass `vector_ip_ops`.
    InnerProduct,
}

impl VectorMetric {
    /// The Postgres operator pgvector registers for this metric.
    pub const fn sql_operator(self) -> &'static str {
        match self {
            Self::L2 => "<->",
            Self::Cosine => "<=>",
            Self::InnerProduct => "<#>",
        }
    }

    /// Map a pgvector `opclass` string, as used in `@@index([...],
    /// opclass: "vector_l2_ops")` (cratestack#156), to the metric it
    /// indexes. Returns `None` for anything that isn't one of
    /// pgvector's three recognized vector opclasses (including a
    /// non-vector opclass on an unrelated index).
    pub fn from_opclass(opclass: &str) -> Option<Self> {
        match opclass {
            "vector_l2_ops" => Some(Self::L2),
            "vector_cosine_ops" => Some(Self::Cosine),
            "vector_ip_ops" => Some(Self::InnerProduct),
            _ => None,
        }
    }
}

/// `<column> <metric op> <query_vector> <cmp> <value>` — a distance-to-
/// a-query-vector expression compared against a bound threshold. Built
/// via [`super::field_ref_ext`]'s `FieldRef::distance_to`, then a
/// comparator method turns it into a [`FilterExpr`]. Mirrors
/// [`super::CoalesceFilter`]'s shape: a left-hand computed expression
/// plus a bound right-hand value.
///
/// PG-only (pgvector) — the embedded rusqlite backend doesn't ship
/// pgvector, so its renderer fails loud, mirroring `FilterExpr::Spatial`.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorDistanceFilter {
    pub column: &'static str,
    pub metric: VectorMetric,
    pub query_vector: Vec<f32>,
    pub op: FilterOp,
    pub value: FilterValue,
}

/// Builder returned by `FieldRef::distance_to` — chain a comparator
/// (`.lt`/`.lte`/`.gt`/`.gte`/`.eq`) for a threshold filter, or `.asc`/
/// `.desc` to use it as an `ORDER BY` target. The common k-NN "closest
/// first" case is `.asc()`; see also `FieldRef::order_by_distance`,
/// sugar for exactly that.
#[derive(Debug, Clone)]
pub struct VectorDistanceExpr {
    column: &'static str,
    metric: VectorMetric,
    query_vector: Vec<f32>,
}

impl VectorDistanceExpr {
    pub(super) fn new(column: &'static str, metric: VectorMetric, query_vector: Vec<f32>) -> Self {
        Self {
            column,
            metric,
            query_vector,
        }
    }

    fn into_filter<V: IntoSqlValue>(self, op: FilterOp, value: V) -> FilterExpr {
        FilterExpr::VectorDistance(VectorDistanceFilter {
            column: self.column,
            metric: self.metric,
            query_vector: self.query_vector,
            op,
            value: FilterValue::Single(value.into_sql_value()),
        })
    }

    pub fn lt<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.into_filter(FilterOp::Lt, value)
    }

    pub fn lte<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.into_filter(FilterOp::Lte, value)
    }

    pub fn gt<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.into_filter(FilterOp::Gt, value)
    }

    pub fn gte<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.into_filter(FilterOp::Gte, value)
    }

    pub fn eq<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.into_filter(FilterOp::Eq, value)
    }

    /// Order by distance, nearest first (`ASC`) — the standard k-NN
    /// shape. Equivalent to `FieldRef::order_by_distance`.
    pub fn asc(self) -> OrderClause {
        self.order(SortDirection::Asc)
    }

    /// Order by distance, farthest first (`DESC`) — e.g. diverse /
    /// furthest-point sampling.
    pub fn desc(self) -> OrderClause {
        self.order(SortDirection::Desc)
    }

    fn order(self, direction: SortDirection) -> OrderClause {
        OrderClause {
            target: OrderTarget::VectorDistance {
                column: self.column,
                metric: self.metric,
                query_vector: self.query_vector,
            },
            direction,
            null_order: NullOrder::Last,
        }
    }
}

#[cfg(test)]
mod tests;
