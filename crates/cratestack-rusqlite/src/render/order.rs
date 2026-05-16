//! `ORDER BY` clause rendering. Column targets and relation-scalar
//! sub-selects share the same NULLS ordering tail.

use std::fmt::Write;

use cratestack_sql::{NullOrder, OrderClause, OrderTarget, SortDirection};

pub(super) fn render_order_clause(clause: &OrderClause, sql: &mut String) {
    match &clause.target {
        OrderTarget::Column(column) => {
            let _ = write!(
                sql,
                "{column} {} {}",
                sort_dir(clause.direction),
                null_order(clause.null_order),
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
                "(SELECT {value_sql} FROM {related_table} WHERE {related_table}.{related_column} = {parent_table}.{parent_column} LIMIT 1) {} {}",
                sort_dir(clause.direction),
                null_order(clause.null_order),
            );
        }
    }
}

fn sort_dir(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
}

fn null_order(order: NullOrder) -> &'static str {
    match order {
        NullOrder::First => "NULLS FIRST",
        NullOrder::Last => "NULLS LAST",
    }
}
