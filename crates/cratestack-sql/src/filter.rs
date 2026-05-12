use std::marker::PhantomData;

pub use cratestack_policy::RelationQuantifier;

use crate::{IntoSqlValue, OrderClause, SortDirection, SqlValue, order::OrderTarget, values::FilterValue};

#[derive(Debug, Clone, Copy)]
pub struct FieldRef<M, T> {
    column: &'static str,
    _marker: PhantomData<fn() -> (M, T)>,
}

impl<M, T> FieldRef<M, T> {
    pub const fn new(column: &'static str) -> Self {
        Self {
            column,
            _marker: PhantomData,
        }
    }

    pub fn asc(self) -> OrderClause {
        OrderClause {
            target: OrderTarget::Column(self.column),
            direction: SortDirection::Asc,
        }
    }

    pub fn desc(self) -> OrderClause {
        OrderClause {
            target: OrderTarget::Column(self.column),
            direction: SortDirection::Desc,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub column: &'static str,
    pub op: FilterOp,
    pub value: FilterValue,
}

impl Filter {
    fn single<V>(column: &'static str, op: FilterOp, value: V) -> Self
    where
        V: IntoSqlValue,
    {
        Self {
            column,
            op,
            value: FilterValue::Single(value.into_sql_value()),
        }
    }

    fn string_pattern(
        column: &'static str,
        op: FilterOp,
        pattern: &str,
        value: impl Into<String>,
    ) -> Self {
        Self {
            column,
            op,
            value: FilterValue::Single(SqlValue::String(pattern.replacen("{}", &value.into(), 1))),
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
    Contains,
    StartsWith,
    IsNull,
    IsNotNull,
}
