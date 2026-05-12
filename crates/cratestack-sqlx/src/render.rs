//! Preview-mode SQL rendering for the Postgres backend.
//!
//! Used by the `preview_sql` / `preview_scoped_sql` helpers — the actual
//! `.run()` path goes through `query/support.rs` and pushes into a
//! `sqlx::QueryBuilder`. The traversal logic itself lives in
//! [`cratestack_sql::render`] (filter + order) and
//! [`cratestack_sql::policy_render`] (policy expressions); this module
//! glues a `PostgresDialect`-backed `StringSink` to those entry points.

use std::fmt::Write;

use cratestack_core::CoolContext;
use cratestack_sql::{
    policy_render::render_action_policy,
    render::{render_filter_expr, render_filter_exprs, render_order_clause, SqlSink},
    OrderClause, PostgresDialect, StringSink,
};

use crate::{FilterExpr, ModelDescriptor, ReadPolicy};

pub(crate) fn render_scoped_select_sql<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
    ctx: &CoolContext,
) -> String {
    let mut sql = format!(
        "SELECT {} FROM {}",
        descriptor.select_projection(),
        descriptor.table_name,
    );
    let dialect = PostgresDialect;

    // Render the WHERE clause into a temp buffer with its own bind counter
    // so we can decide whether to emit `WHERE` at all without committing to
    // a layout up front. Bind numbering picks up where this leaves off.
    let mut where_sql = String::new();
    let mut bind_index = 1usize;
    {
        let mut sink = StringSink::new(&mut where_sql, &dialect, bind_index);
        if !filters.is_empty() {
            render_filter_exprs(&mut sink, filters);
        }
        let has_user_clause = !filters.is_empty();
        let has_policy = !descriptor.read_allow_policies.is_empty()
            || !descriptor.read_deny_policies.is_empty();
        if has_user_clause && has_policy {
            sink.push_sql(" AND (");
        }
        if has_policy {
            render_action_policy(
                &mut sink,
                descriptor.read_allow_policies,
                descriptor.read_deny_policies,
                ctx,
            );
        }
        if has_user_clause && has_policy {
            sink.push_sql(")");
        }
        bind_index = sink.bind_index();
    }

    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }

    if !order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        for (index, clause) in order_by.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let mut sink = StringSink::new(&mut sql, &dialect, bind_index);
            render_order_clause(&mut sink, clause);
            bind_index = sink.bind_index();
        }
    }

    match (limit, offset) {
        (Some(_), Some(_)) => {
            let _ = write!(sql, " LIMIT ${bind_index} OFFSET ${}", bind_index + 1);
        }
        (Some(_), None) => {
            let _ = write!(sql, " LIMIT ${bind_index}");
        }
        (None, Some(_)) => {
            let _ = write!(sql, " OFFSET ${bind_index}");
        }
        (None, None) => {}
    }

    sql
}

pub(crate) fn render_read_policy_sql(
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
    bind_index: &mut usize,
) -> Option<String> {
    if allow_policies.is_empty() {
        return Some("FALSE".to_owned());
    }
    let dialect = PostgresDialect;
    let mut sql = String::new();
    let mut sink = StringSink::new(&mut sql, &dialect, *bind_index);
    render_action_policy(&mut sink, allow_policies, deny_policies, ctx);
    *bind_index = sink.bind_index();
    Some(sql)
}

/// Render a filter sub-expression into an existing SQL buffer using the
/// caller's running bind index. Used by `FindMany::preview_sql` to build
/// its WHERE clause without re-implementing the traversal.
pub(crate) fn render_filter_expr_sql(
    filter: &FilterExpr,
    sql: &mut String,
    bind_index: &mut usize,
) {
    let dialect = PostgresDialect;
    let mut sink = StringSink::new(sql, &dialect, *bind_index);
    render_filter_expr(&mut sink, filter);
    *bind_index = sink.bind_index();
}

pub(crate) fn render_order_clause_sql(clause: &OrderClause, sql: &mut String) {
    let dialect = PostgresDialect;
    let mut sink = StringSink::new(sql, &dialect, 1);
    render_order_clause(&mut sink, clause);
}
