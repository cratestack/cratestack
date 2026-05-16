use crate::{IntoSqlValue, values::FilterValue};

use super::expr::FilterExpr;
use super::field_ref::FieldRef;
use super::op::FilterOp;

/// `COALESCE(col_a, col_b, ...) <op> <value>` — left-hand expression
/// is the first non-null among the listed columns; right-hand side is
/// a bound value via the usual `FilterValue` envelope. Lets schemas
/// express the "ranked-fallback compare" pattern that shows up in
/// outbox / scheduler tables, where a single row carries several
/// time columns and the dispatcher wants the earliest non-null one.
///
/// `IsNull` and `IsNotNull` are valid `op` choices too: a row where
/// every coalesced column is null collapses to `COALESCE(...) IS
/// NULL`, which the engine can index-elide when at least one of the
/// inputs has a `NOT NULL` constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct CoalesceFilter {
    pub columns: Vec<&'static str>,
    pub op: FilterOp,
    pub value: FilterValue,
}

/// Anything that can name a single SQL column. Lets [`coalesce`]
/// accept both bare `&'static str` column names and typed
/// [`FieldRef`] handles, so callers don't have to choose between
/// schema-rooted typing and ad-hoc strings at the call site.
pub trait IntoColumnName {
    fn into_column_name(self) -> &'static str;
}

impl IntoColumnName for &'static str {
    fn into_column_name(self) -> &'static str {
        self
    }
}

impl<M, T> IntoColumnName for FieldRef<M, T> {
    fn into_column_name(self) -> &'static str {
        self.column_name()
    }
}

/// Build a `COALESCE(...)` left-hand operand. The returned
/// [`CoalesceExpr`] carries the column list; chain a comparator
/// method (`.lte`, `.eq`, `.is_null`, ...) to produce a [`FilterExpr`]
/// the query builders can consume.
///
/// ```ignore
/// .where_(coalesce([
///     task::next_attempt_at(),
///     task::scheduled_at(),
///     task::created_at(),
/// ]).lte(now))
/// ```
pub fn coalesce<I, C>(columns: I) -> CoalesceExpr
where
    I: IntoIterator<Item = C>,
    C: IntoColumnName,
{
    CoalesceExpr {
        columns: columns
            .into_iter()
            .map(IntoColumnName::into_column_name)
            .collect(),
    }
}

/// Left-hand operand of a coalesce-based filter — chain a comparator
/// method to turn it into a [`FilterExpr`].
#[derive(Debug, Clone)]
pub struct CoalesceExpr {
    columns: Vec<&'static str>,
}

impl CoalesceExpr {
    fn into_filter<V: IntoSqlValue>(self, op: FilterOp, value: V) -> FilterExpr {
        FilterExpr::Coalesce(CoalesceFilter {
            columns: self.columns,
            op,
            value: FilterValue::Single(value.into_sql_value()),
        })
    }

    pub fn eq<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.into_filter(FilterOp::Eq, value)
    }
    pub fn ne<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.into_filter(FilterOp::Ne, value)
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

    /// `COALESCE(...) IS NULL` — every input column was null. No
    /// bind: this side never carries a value.
    pub fn is_null(self) -> FilterExpr {
        FilterExpr::Coalesce(CoalesceFilter {
            columns: self.columns,
            op: FilterOp::IsNull,
            value: FilterValue::None,
        })
    }

    /// `COALESCE(...) IS NOT NULL` — at least one input column has a
    /// value.
    pub fn is_not_null(self) -> FilterExpr {
        FilterExpr::Coalesce(CoalesceFilter {
            columns: self.columns,
            op: FilterOp::IsNotNull,
            value: FilterValue::None,
        })
    }
}
