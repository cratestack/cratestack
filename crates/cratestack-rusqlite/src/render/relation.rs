//! `RelationFilter` rendering. Same `EXISTS` / `NOT EXISTS` shapes as the
//! sqlx backend; embedded layer has no policy threading on the related
//! table, so the recursion just defers to `render_filter_expr`.

use std::fmt::Write;

use cratestack_sql::{Dialect, RelationFilter, RelationQuantifier, SqlValue};

use super::filter::render_filter_expr;

pub(super) fn render_relation(
    dialect: &dyn Dialect,
    relation: &RelationFilter,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    match relation.quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => {
            let _ = write!(
                sql,
                "EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND ",
                relation.related_table,
                relation.related_table,
                relation.related_column,
                relation.parent_table,
                relation.parent_column,
            );
            render_filter_expr(dialect, &relation.filter, sql, binds, bind_index);
            sql.push(')');
        }
        RelationQuantifier::None => {
            let _ = write!(
                sql,
                "NOT EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND ",
                relation.related_table,
                relation.related_table,
                relation.related_column,
                relation.parent_table,
                relation.parent_column,
            );
            render_filter_expr(dialect, &relation.filter, sql, binds, bind_index);
            sql.push(')');
        }
        RelationQuantifier::Every => {
            let _ = write!(
                sql,
                "NOT EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND NOT (",
                relation.related_table,
                relation.related_table,
                relation.related_column,
                relation.parent_table,
                relation.parent_column,
            );
            render_filter_expr(dialect, &relation.filter, sql, binds, bind_index);
            sql.push_str("))");
        }
    }
}
