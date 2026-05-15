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

    /// The underlying SQL column name. Exposed so AST-builder helpers
    /// like [`coalesce`] can interop with the typed `FieldRef` API
    /// without giving up generic-column flexibility.
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

    /// PG: `col ? 'key'` — the JSON document contains `key` as a
    /// top-level field. SQLite (no native `?` operator): lowers to
    /// `json_extract(col, '$.key') IS NOT NULL`.
    ///
    /// Intended for `jsonb` / JSON columns. Using this on a non-JSON
    /// column compiles fine but errors at the engine layer when the
    /// SQL runs — Rust's type system doesn't gate this for you.
    pub fn json_has_key(self, key: &'static str) -> FilterExpr {
        FilterExpr::Json(JsonFilter::HasKey {
            column: self.column,
            key,
        })
    }

    /// PG: `col ->> 'key' <op> $1` — extract the value at `key` as
    /// text, then compare. SQLite: `json_extract(col, '$.key') <op>
    /// $1`. Returns a [`JsonTextPath`] that supports the standard
    /// comparison ops via chained methods.
    pub fn json_get_text(self, key: &'static str) -> JsonTextPath {
        JsonTextPath {
            column: self.column,
            key,
        }
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

#[derive(Debug, Clone, PartialEq)]
pub enum FilterExpr {
    Filter(Filter),
    All(Vec<FilterExpr>),
    Any(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
    Relation(RelationFilter),
    /// `COALESCE(col_a, col_b, ...) op value` — built via [`coalesce`].
    Coalesce(CoalesceFilter),
    /// JSON / JSONB column predicates — see [`JsonFilter`]. Built via
    /// `FieldRef::json_has_key(...)` and
    /// `FieldRef::json_get_text(...).<cmp>(...)`.
    Json(JsonFilter),
}

/// JSON / JSONB filter predicates. Two flavors:
///
/// * `HasKey` — `col ? 'key'` on PG (key-exists operator). On SQLite
///   this lowers to `json_extract(col, '$.key') IS NOT NULL`, which
///   has the same matches-some-non-null-value semantics for the most
///   common case (records where the schema sometimes carries a key,
///   sometimes doesn't); JSON values explicitly stored as `null`
///   diverge between backends, mirroring the operators themselves.
/// * `GetText` — `col ->> 'key' <op> $1` on PG (extract-as-text +
///   compare). On SQLite the same `json_extract` path with a column
///   accessor handles it. Supported comparison ops are the standard
///   `Eq/Ne/Lt/Lte/Gt/Gte` plus `IsNull` / `IsNotNull`.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonFilter {
    HasKey {
        column: &'static str,
        key: &'static str,
    },
    GetText {
        column: &'static str,
        key: &'static str,
        op: FilterOp,
        value: FilterValue,
    },
}

/// Left-hand operand of a `json_get_text` filter — chain a comparison
/// method (`.eq`, `.lt`, `.is_null`, ...) to produce a [`FilterExpr`].
#[derive(Debug, Clone, Copy)]
pub struct JsonTextPath {
    column: &'static str,
    key: &'static str,
}

impl JsonTextPath {
    fn binary<V: IntoSqlValue>(self, op: FilterOp, value: V) -> FilterExpr {
        FilterExpr::Json(JsonFilter::GetText {
            column: self.column,
            key: self.key,
            op,
            value: FilterValue::Single(value.into_sql_value()),
        })
    }

    pub fn eq<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Eq, value)
    }
    pub fn ne<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Ne, value)
    }
    pub fn lt<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Lt, value)
    }
    pub fn lte<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Lte, value)
    }
    pub fn gt<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Gt, value)
    }
    pub fn gte<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Gte, value)
    }

    /// `col ->> 'key' IS NULL` — the JSON document either lacks the
    /// key, or stores it as JSON null. (PG and SQLite agree here.)
    pub fn is_null(self) -> FilterExpr {
        FilterExpr::Json(JsonFilter::GetText {
            column: self.column,
            key: self.key,
            op: FilterOp::IsNull,
            value: FilterValue::None,
        })
    }

    /// `col ->> 'key' IS NOT NULL` — the JSON document has the key
    /// with a non-null primitive value. Note: a PG `?` test (use
    /// [`FieldRef::json_has_key`]) treats JSON null as a present key
    /// where this method does not.
    pub fn is_not_null(self) -> FilterExpr {
        FilterExpr::Json(JsonFilter::GetText {
            column: self.column,
            key: self.key,
            op: FilterOp::IsNotNull,
            value: FilterValue::None,
        })
    }
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
        self.column
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
    /// `(col IS NULL OR col = $1)` — for the "nullable column matches
    /// either the bound value or null" pattern that's otherwise
    /// awkward to express via `Any([is_null, eq])` (the latter
    /// double-binds the value when the same caller wants the null-
    /// punning behavior elsewhere). Single-bind, single op.
    EqOrNull,
}
