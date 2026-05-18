//! Top-level `SELECT` SQL rendering for scoped reads — combines the
//! caller's filters, the model's read policy, ORDER BY, and pagination.

use std::fmt::Write;

use cratestack_core::CoolContext;
use cratestack_sql::ReadSource;

use crate::{FilterExpr, OrderClause};

use super::filter::render_filter_sql;
use super::order::render_order_clause_sql;
use super::policy::render_read_policy_sql;

pub(crate) fn render_scoped_select_sql<M, PK>(
    descriptor: &dyn ReadSource<M, PK>,
    filters: &[FilterExpr],
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
    ctx: &CoolContext,
) -> String {
    let mut sql = format!(
        "SELECT {} FROM {}",
        descriptor.select_projection(),
        descriptor.table_name(),
    );
    let mut bind_index = 1usize;
    let user_clause = render_filter_sql(filters, &mut bind_index);
    let policy_clause = render_read_policy_sql(
        descriptor.read_allow_policies(),
        descriptor.read_deny_policies(),
        ctx,
        &mut bind_index,
    );

    match (user_clause, policy_clause) {
        (Some(user_clause), Some(policy_clause)) => {
            let _ = write!(sql, " WHERE {user_clause} AND ({policy_clause})");
        }
        (Some(user_clause), None) => {
            let _ = write!(sql, " WHERE {user_clause}");
        }
        (None, Some(policy_clause)) => {
            let _ = write!(sql, " WHERE {policy_clause}");
        }
        (None, None) => {}
    }

    if !order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        for (index, clause) in order_by.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            render_order_clause_sql(clause, &mut sql);
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
