//! `ORDER BY` + `LIMIT/OFFSET` pushers and the trivial direction/null-
//! ordering keyword helpers.

use cratestack_sql::OrderTarget;

use crate::{OrderClause, SortDirection, sqlx};

pub(crate) fn push_order_and_paging(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
) {
    if !order_by.is_empty() {
        query.push(" ORDER BY ");
        for (index, clause) in order_by.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            push_order_clause_query(query, clause);
        }
    }

    match (limit, offset) {
        (Some(limit), Some(offset)) => {
            query.push(" LIMIT ");
            query.push_bind(limit);
            query.push(" OFFSET ");
            query.push_bind(offset);
        }
        (Some(limit), None) => {
            query.push(" LIMIT ");
            query.push_bind(limit);
        }
        (None, Some(offset)) => {
            query.push(" OFFSET ");
            query.push_bind(offset);
        }
        (None, None) => {}
    }
}

fn push_order_clause_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    clause: &OrderClause,
) {
    match &clause.target {
        OrderTarget::Column(column) => {
            query
                .push(*column)
                .push(" ")
                .push(sort_direction_sql(clause.direction))
                .push(" ")
                .push(null_order_sql(clause.null_order));
        }
        OrderTarget::RelationScalar {
            parent_table,
            parent_column,
            related_table,
            related_column,
            value_sql,
        } => {
            query
                .push("(SELECT ")
                .push(*value_sql)
                .push(" FROM ")
                .push(*related_table)
                .push(" WHERE ")
                .push(*related_table)
                .push(".")
                .push(*related_column)
                .push(" = ")
                .push(*parent_table)
                .push(".")
                .push(*parent_column)
                .push(" LIMIT 1) ")
                .push(sort_direction_sql(clause.direction))
                .push(" ")
                .push(null_order_sql(clause.null_order));
        }
    }
}

pub(crate) fn sort_direction_sql(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
}

pub(crate) fn null_order_sql(order: cratestack_sql::NullOrder) -> &'static str {
    match order {
        cratestack_sql::NullOrder::First => "NULLS FIRST",
        cratestack_sql::NullOrder::Last => "NULLS LAST",
    }
}
