//! `ORDER BY` clause SQL rendering — scalar columns + relation-scalar
//! `(SELECT ...)` subselects, with direction + null-ordering suffixes.

use std::fmt::Write;

use cratestack_sql::OrderTarget;

use crate::{OrderClause, SortDirection};

pub(crate) fn render_order_clause_sql(clause: &OrderClause, sql: &mut String) {
    match &clause.target {
        OrderTarget::Column(column) => {
            let _ = write!(
                sql,
                "{} {} {}",
                column,
                sort_direction_sql(clause.direction),
                null_order_sql(clause.null_order),
            );
        }
        OrderTarget::RelationScalar {
            parent_table,
            parent_column,
            related_table,
            related_column,
            value_sql,
        } => {
            let _ = write!(
                sql,
                "(SELECT {} FROM {} WHERE {}.{} = {}.{} LIMIT 1) {} {}",
                value_sql,
                related_table,
                related_table,
                related_column,
                parent_table,
                parent_column,
                sort_direction_sql(clause.direction),
                null_order_sql(clause.null_order),
            );
        }
    }
}

fn sort_direction_sql(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
}

fn null_order_sql(order: cratestack_sql::NullOrder) -> &'static str {
    match order {
        cratestack_sql::NullOrder::First => "NULLS FIRST",
        cratestack_sql::NullOrder::Last => "NULLS LAST",
    }
}
