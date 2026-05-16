use std::marker::PhantomData;

use crate::{IntoSqlValue, OrderClause, SortDirection, order::OrderTarget, values::FilterValue};

use super::filter::Filter;
use super::op::FilterOp;

#[derive(Debug, Clone, Copy)]
pub struct FieldRef<M, T> {
    pub(super) column: &'static str,
    _marker: PhantomData<fn() -> (M, T)>,
}

impl<M, T> FieldRef<M, T> {
    pub const fn new(column: &'static str) -> Self {
        Self {
            column,
            _marker: PhantomData,
        }
    }

    /// The underlying SQL column name. Exposed so AST-builder helpers
    /// like [`super::coalesce::coalesce`] can interop with the typed
    /// `FieldRef` API without giving up generic-column flexibility.
    pub const fn column_name(self) -> &'static str {
        self.column
    }

    pub fn asc(self) -> OrderClause {
        OrderClause {
            target: OrderTarget::Column(self.column),
            direction: SortDirection::Asc,
            null_order: crate::NullOrder::Last,
        }
    }

    pub fn desc(self) -> OrderClause {
        OrderClause {
            target: OrderTarget::Column(self.column),
            direction: SortDirection::Desc,
            null_order: crate::NullOrder::Last,
        }
    }
}

impl<M, T> FieldRef<M, T> {
    pub fn eq<V>(self, value: V) -> Filter
    where
        V: IntoSqlValue,
    {
        Filter::single(self.column, FilterOp::Eq, value)
    }

    pub fn ne<V>(self, value: V) -> Filter
    where
        V: IntoSqlValue,
    {
        Filter::single(self.column, FilterOp::Ne, value)
    }

    pub fn in_<I, V>(self, values: I) -> Filter
    where
        I: IntoIterator<Item = V>,
        V: IntoSqlValue,
    {
        Filter {
            column: self.column,
            op: FilterOp::In,
            value: FilterValue::Many(
                values
                    .into_iter()
                    .map(IntoSqlValue::into_sql_value)
                    .collect(),
            ),
        }
    }

    pub fn lt<V>(self, value: V) -> Filter
    where
        V: IntoSqlValue,
    {
        Filter::single(self.column, FilterOp::Lt, value)
    }

    pub fn lte<V>(self, value: V) -> Filter
    where
        V: IntoSqlValue,
    {
        Filter::single(self.column, FilterOp::Lte, value)
    }

    pub fn gt<V>(self, value: V) -> Filter
    where
        V: IntoSqlValue,
    {
        Filter::single(self.column, FilterOp::Gt, value)
    }

    pub fn gte<V>(self, value: V) -> Filter
    where
        V: IntoSqlValue,
    {
        Filter::single(self.column, FilterOp::Gte, value)
    }

    /// Match rows where the column is null OR equals `value`. The
    /// canonical inline-SQL workaround for "filter only if the caller
    /// provided this value, otherwise let nulls through" — schemas
    /// with sparse, optional foreign-key-style columns hit this
    /// constantly. Renders as `(col IS NULL OR col = $1)`.
    ///
    /// Use [`Self::match_optional`] when the *caller's* value is
    /// itself an `Option` — that variant skips the filter entirely on
    /// `None` instead of binding a null.
    pub fn eq_or_null<V>(self, value: V) -> Filter
    where
        V: IntoSqlValue,
    {
        Filter::single(self.column, FilterOp::EqOrNull, value)
    }

    /// Filter on equality when the caller has a value, skip the
    /// filter entirely when they don't. Returns `None` for the no-op
    /// case so callers can plumb it through
    /// [`crate::FilterExpr::any_of_optional`]-style helpers, or feed
    /// it directly into a `where_optional(...)` builder method on the
    /// query builders.
    ///
    /// The emitted filter is the same `(col IS NULL OR col = $1)` as
    /// [`Self::eq_or_null`] — when the caller *did* supply a value,
    /// we still let nulls through, matching the canonical
    /// "optional-equality with null-as-wildcard" semantics from the
    /// inline-SQL pattern.
    pub fn match_optional<V>(self, value: Option<V>) -> Option<Filter>
    where
        V: IntoSqlValue,
    {
        value.map(|v| self.eq_or_null(v))
    }
}

impl<M> FieldRef<M, bool> {
    pub fn is_true(self) -> Filter {
        self.eq(true)
    }

    pub fn is_false(self) -> Filter {
        self.eq(false)
    }
}

impl<M> FieldRef<M, String> {
    pub fn contains(self, value: impl Into<String>) -> Filter {
        Filter::string_pattern(self.column, FilterOp::Contains, "%{}%", value)
    }

    pub fn starts_with(self, value: impl Into<String>) -> Filter {
        Filter::string_pattern(self.column, FilterOp::StartsWith, "{}%", value)
    }
}

impl<M, T> FieldRef<M, Option<T>> {
    pub fn is_null(self) -> Filter {
        Filter {
            column: self.column,
            op: FilterOp::IsNull,
            value: FilterValue::None,
        }
    }

    pub fn is_not_null(self) -> Filter {
        Filter {
            column: self.column,
            op: FilterOp::IsNotNull,
            value: FilterValue::None,
        }
    }
}

impl<M> FieldRef<M, Option<String>> {
    pub fn contains(self, value: impl Into<String>) -> Filter {
        Filter::string_pattern(self.column, FilterOp::Contains, "%{}%", value)
    }

    pub fn starts_with(self, value: impl Into<String>) -> Filter {
        Filter::string_pattern(self.column, FilterOp::StartsWith, "{}%", value)
    }
}
