//! Backend-agnostic SQL rendering for the shared filter/order ASTs.
//!
//! The two server backends (`cratestack-sqlx` and `cratestack-rusqlite`) used
//! to traverse `FilterExpr`, `OrderClause`, and the policy AST themselves —
//! and `cratestack-sqlx` did the traversal twice, once for `preview_sql` (raw
//! `$N` placeholders into a `String`) and once for `.run()` (binds plus
//! placeholders pushed into a `sqlx::QueryBuilder`). Same enum match, same
//! operator strings, same `EXISTS (SELECT 1 FROM …)` relation shape — three
//! parallel copies in total.
//!
//! The fix is the [`SqlSink`] trait: an output abstraction that decides per
//! impl whether a `push_bind` call emits a placeholder, collects a value into
//! a `Vec<SqlValue>`, or simply hands the value to a backend's bind machinery
//! (such as `sqlx::QueryBuilder::push_bind`). Each backend supplies its own
//! `SqlSink` impl; the traversal lives here exactly once.

use std::fmt::Write as _;

use crate::{
    Dialect, FilterExpr, FilterOp, FilterValue, OrderClause, OrderTarget, RelationFilter,
    RelationQuantifier, SortDirection, SqlValue,
};

/// Output sink the renderers write into. Implementations decide what
/// happens on `push_bind`: emit a numbered placeholder (`StringSink`), push
/// into a `sqlx::QueryBuilder`, or both. The renderers themselves never
/// know.
pub trait SqlSink {
    /// Append a literal SQL fragment (column names, operators, keywords).
    /// Implementations must not bind values here.
    fn push_sql(&mut self, sql: &str);

    /// Record a value to be bound at the current position. The sink decides
    /// whether to emit a placeholder, push the value into an external bind
    /// list, or both.
    fn push_bind(&mut self, value: &SqlValue);
}

impl SqlSink for &mut dyn SqlSink {
    fn push_sql(&mut self, sql: &str) {
        (**self).push_sql(sql);
    }

    fn push_bind(&mut self, value: &SqlValue) {
        (**self).push_bind(value);
    }
}

/// `SqlSink` that writes into an owned `String` buffer with dialect-driven
/// numbered placeholders. Optionally also collects bound values into a
/// `Vec<SqlValue>` — `cratestack-rusqlite` enables this so the rendered SQL
/// and its bind order travel together to `rusqlite::Statement`; the sqlx
/// `preview_sql` path leaves the bind sink absent because the output is for
/// human display only.
pub struct StringSink<'a, D: Dialect + ?Sized> {
    sql: &'a mut String,
    dialect: &'a D,
    bind_index: usize,
    binds: Option<&'a mut Vec<SqlValue>>,
}

impl<'a, D: Dialect + ?Sized> StringSink<'a, D> {
    /// Build a sink that emits placeholders but discards bind values. Use
    /// when the caller only needs the rendered SQL text (e.g. `preview_sql`).
    pub fn new(sql: &'a mut String, dialect: &'a D, start_bind_index: usize) -> Self {
        Self {
            sql,
            dialect,
            bind_index: start_bind_index,
            binds: None,
        }
    }

    /// Build a sink that emits placeholders AND collects bind values into the
    /// supplied `Vec`. Bind ordering matches placeholder emission order.
    pub fn with_binds(
        sql: &'a mut String,
        dialect: &'a D,
        start_bind_index: usize,
        binds: &'a mut Vec<SqlValue>,
    ) -> Self {
        Self {
            sql,
            dialect,
            bind_index: start_bind_index,
            binds: Some(binds),
        }
    }

    /// Index of the next placeholder this sink will emit. Backends that
    /// continue rendering after the shared helpers return use this to keep
    /// numbering monotonic.
    pub fn bind_index(&self) -> usize {
        self.bind_index
    }
}

impl<'a, D: Dialect + ?Sized> SqlSink for StringSink<'a, D> {
    fn push_sql(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    fn push_bind(&mut self, value: &SqlValue) {
        self.dialect.write_placeholder(self.sql, self.bind_index);
        self.bind_index += 1;
        if let Some(binds) = self.binds.as_deref_mut() {
            binds.push(value.clone());
        }
    }
}

/// Render a slice of top-level filter expressions joined by `AND`. Emits
/// nothing when the slice is empty. Caller is responsible for any
/// surrounding `WHERE`.
pub fn render_filter_exprs<S: SqlSink + ?Sized>(sink: &mut S, filters: &[FilterExpr]) {
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            sink.push_sql(" AND ");
        }
        render_filter_expr(sink, filter);
    }
}

/// Render a single filter expression (terminal, conjunction, disjunction,
/// negation, or relation predicate) into the sink.
pub fn render_filter_expr<S: SqlSink + ?Sized>(sink: &mut S, filter: &FilterExpr) {
    match filter {
        FilterExpr::Filter(filter) => match filter.op {
            FilterOp::Eq => render_binary(sink, filter.column, "=", &filter.value),
            FilterOp::Ne => render_binary(sink, filter.column, "!=", &filter.value),
            FilterOp::Lt => render_binary(sink, filter.column, "<", &filter.value),
            FilterOp::Lte => render_binary(sink, filter.column, "<=", &filter.value),
            FilterOp::Gt => render_binary(sink, filter.column, ">", &filter.value),
            FilterOp::Gte => render_binary(sink, filter.column, ">=", &filter.value),
            FilterOp::In => {
                let FilterValue::Many(values) = &filter.value else {
                    unreachable!("FilterOp::In requires FilterValue::Many");
                };
                sink.push_sql(filter.column);
                sink.push_sql(" IN (");
                for (idx, value) in values.iter().enumerate() {
                    if idx > 0 {
                        sink.push_sql(", ");
                    }
                    sink.push_bind(value);
                }
                sink.push_sql(")");
            }
            FilterOp::Contains | FilterOp::StartsWith => {
                render_binary(sink, filter.column, "LIKE", &filter.value)
            }
            FilterOp::IsNull => {
                sink.push_sql(filter.column);
                sink.push_sql(" IS NULL");
            }
            FilterOp::IsNotNull => {
                sink.push_sql(filter.column);
                sink.push_sql(" IS NOT NULL");
            }
        },
        FilterExpr::All(filters) => render_grouped(sink, filters, " AND "),
        FilterExpr::Any(filters) => render_grouped(sink, filters, " OR "),
        FilterExpr::Not(filter) => {
            sink.push_sql("NOT (");
            render_filter_expr(sink, filter);
            sink.push_sql(")");
        }
        FilterExpr::Relation(relation) => render_relation_filter(sink, relation),
    }
}

/// Render a relation filter (`some`, `every`, `none`, `to_one`) as the
/// appropriate `EXISTS` / `NOT EXISTS` correlated subquery.
pub fn render_relation_filter<S: SqlSink + ?Sized>(sink: &mut S, relation: &RelationFilter) {
    render_relation_subquery(
        sink,
        relation.quantifier,
        relation.parent_table,
        relation.parent_column,
        relation.related_table,
        relation.related_column,
        &|sink| render_filter_expr(sink, &relation.filter),
    );
}

/// Shared `EXISTS (SELECT 1 FROM <related> WHERE <related>.<col> =
/// <parent>.<col> AND <predicate>)` template used by both relation filters
/// and relation policies. The predicate is supplied by the caller so the
/// same template handles both `FilterExpr` subtrees and `PolicyExpr`
/// subtrees.
pub fn render_relation_subquery<S, F>(
    sink: &mut S,
    quantifier: RelationQuantifier,
    parent_table: &str,
    parent_column: &str,
    related_table: &str,
    related_column: &str,
    predicate: &F,
) where
    S: SqlSink + ?Sized,
    F: Fn(&mut S),
{
    let (prefix, suffix) = match quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => ("EXISTS (SELECT 1 FROM ", ")"),
        RelationQuantifier::None => ("NOT EXISTS (SELECT 1 FROM ", ")"),
        RelationQuantifier::Every => ("NOT EXISTS (SELECT 1 FROM ", "))"),
    };
    sink.push_sql(prefix);
    sink.push_sql(related_table);
    sink.push_sql(" WHERE ");
    sink.push_sql(related_table);
    sink.push_sql(".");
    sink.push_sql(related_column);
    sink.push_sql(" = ");
    sink.push_sql(parent_table);
    sink.push_sql(".");
    sink.push_sql(parent_column);
    match quantifier {
        RelationQuantifier::Every => sink.push_sql(" AND NOT ("),
        _ => sink.push_sql(" AND "),
    }
    predicate(sink);
    sink.push_sql(suffix);
}

/// Render a single `ORDER BY` clause. Caller emits the `ORDER BY` keyword
/// and joins multiple clauses with `, `.
///
/// Plain-column ordering does NOT append a `NULLS LAST` qualifier — the
/// pre-refactor `cratestack-sqlx` renderer omitted it (Postgres's default
/// ASC null ordering happens to be `NULLS LAST` already) and several tests
/// pin that exact spelling. Relation-scalar ordering keeps the explicit
/// `NULLS LAST` because the subquery wrapper exposes the null differently
/// depending on row presence and the explicit qualifier nails the
/// behaviour.
pub fn render_order_clause<S: SqlSink + ?Sized>(sink: &mut S, clause: &OrderClause) {
    match &clause.target {
        OrderTarget::Column(column) => {
            sink.push_sql(column);
            sink.push_sql(" ");
            sink.push_sql(sort_direction_sql(clause.direction));
        }
        OrderTarget::RelationScalar {
            parent_table,
            parent_column,
            related_table,
            related_column,
            value_sql,
        } => {
            sink.push_sql("(SELECT ");
            sink.push_sql(value_sql);
            sink.push_sql(" FROM ");
            sink.push_sql(related_table);
            sink.push_sql(" WHERE ");
            sink.push_sql(related_table);
            sink.push_sql(".");
            sink.push_sql(related_column);
            sink.push_sql(" = ");
            sink.push_sql(parent_table);
            sink.push_sql(".");
            sink.push_sql(parent_column);
            sink.push_sql(" LIMIT 1) ");
            sink.push_sql(sort_direction_sql(clause.direction));
            sink.push_sql(" ");
            sink.push_sql(NULL_ORDER_SQL);
        }
    }
}

/// Render the `ORDER BY <…>` and `LIMIT <…> OFFSET <…>` tail of a SELECT.
/// Both halves are optional. Bind values for limit and offset go through the
/// sink so they participate in the dialect's placeholder scheme.
pub fn render_order_and_paging<S: SqlSink + ?Sized>(
    sink: &mut S,
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
) {
    if !order_by.is_empty() {
        sink.push_sql(" ORDER BY ");
        for (index, clause) in order_by.iter().enumerate() {
            if index > 0 {
                sink.push_sql(", ");
            }
            render_order_clause(sink, clause);
        }
    }
    if let Some(limit) = limit {
        sink.push_sql(" LIMIT ");
        sink.push_bind(&SqlValue::Int(limit));
    }
    if let Some(offset) = offset {
        sink.push_sql(" OFFSET ");
        sink.push_bind(&SqlValue::Int(offset));
    }
}

/// SQL text for a sort direction. Public so backends can reuse the spelling
/// outside the renderers (e.g. when assembling synthetic tie-breaker
/// clauses).
pub const fn sort_direction_sql(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
}

/// Null-ordering qualifier appended to every `ORDER BY` clause. `NULLS
/// LAST` matches the existing behaviour on both backends; promoting it to a
/// constant keeps the two renderers consistent and gives a single place to
/// change if a future dialect needs different default semantics.
pub const NULL_ORDER_SQL: &str = "NULLS LAST";

fn render_binary<S: SqlSink + ?Sized>(
    sink: &mut S,
    column: &str,
    operator: &str,
    value: &FilterValue,
) {
    let FilterValue::Single(value) = value else {
        unreachable!("binary filter ops require FilterValue::Single");
    };
    sink.push_sql(column);
    sink.push_sql(" ");
    sink.push_sql(operator);
    sink.push_sql(" ");
    sink.push_bind(value);
}

fn render_grouped<S: SqlSink + ?Sized>(sink: &mut S, filters: &[FilterExpr], joiner: &str) {
    sink.push_sql("(");
    for (idx, filter) in filters.iter().enumerate() {
        if idx > 0 {
            sink.push_sql(joiner);
        }
        render_filter_expr(sink, filter);
    }
    sink.push_sql(")");
}

/// Format a 1-indexed placeholder using the supplied dialect, into a fresh
/// `String`. Useful for backends that need a single placeholder outside the
/// sink-driven traversal (e.g. when assembling a `WHERE pk = $N` clause by
/// hand).
pub fn placeholder_string<D: Dialect + ?Sized>(dialect: &D, index: usize) -> String {
    let mut s = String::new();
    let _ = write!(&mut s, "");
    dialect.write_placeholder(&mut s, index);
    s
}
