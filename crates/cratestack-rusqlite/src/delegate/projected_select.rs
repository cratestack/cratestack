//! Partial-projection SELECT rendering for `ProjectedFindMany`. Reuses
//! the regular filter rendering but swaps the projection list for the
//! descriptor's subset projection.

use std::fmt::Write;

use cratestack_sql::{Dialect, FilterExpr, OrderClause, ReadSource, SqlValue};

pub(super) fn build_partial_select<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &dyn ReadSource<M, PK>,
    selected: &[&'static str],
    filters: &[FilterExpr],
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!(
        "SELECT {} FROM {}",
        descriptor.select_projection_subset(selected),
        descriptor.table_name(),
    );
    let mut binds: Vec<SqlValue> = Vec::new();
    let mut bind_index = 1usize;
    let mut where_sql = String::new();
    let mut wrote = false;
    if let Some(soft_delete) = descriptor.soft_delete_column() {
        let _ = write!(&mut where_sql, "{soft_delete} IS NULL");
        wrote = true;
    }
    if !filters.is_empty() {
        if wrote {
            where_sql.push_str(" AND ");
        }
        let mut joined = false;
        for filter in filters {
            if joined {
                where_sql.push_str(" AND ");
            }
            crate::render::render_filter_expr(
                dialect,
                filter,
                &mut where_sql,
                &mut binds,
                &mut bind_index,
            );
            joined = true;
        }
    }
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }
    if !order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        for (idx, clause) in order_by.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            // Cheap inline rewrite of render_order_clause — it isn't
            // pub from render.rs and we only need the column-target
            // path here in practice. For relation-scalar order in a
            // projection we'd defer; for v1 of `.select(...)` plain
            // column ordering is the common case.
            use cratestack_sql::{OrderTarget, SortDirection};
            match &clause.target {
                OrderTarget::Column(column) => {
                    let direction = match clause.direction {
                        SortDirection::Asc => "ASC",
                        SortDirection::Desc => "DESC",
                    };
                    let nulls = match clause.null_order {
                        cratestack_sql::NullOrder::First => "NULLS FIRST",
                        cratestack_sql::NullOrder::Last => "NULLS LAST",
                    };
                    let _ = write!(&mut sql, "{column} {direction} {nulls}");
                }
                OrderTarget::RelationScalar { .. } => {
                    // Relation-scalar ordering on a projected query is
                    // a non-v1 shape — skip the clause silently rather
                    // than emit something that'd join the relation
                    // table while we're trying to keep the projection
                    // narrow.
                }
            }
        }
    }
    if let Some(limit_value) = limit {
        sql.push_str(" LIMIT ");
        dialect.write_placeholder(&mut sql, bind_index);
        bind_index += 1;
        binds.push(SqlValue::Int(limit_value));
    }
    if let Some(offset_value) = offset {
        sql.push_str(" OFFSET ");
        dialect.write_placeholder(&mut sql, bind_index);
        binds.push(SqlValue::Int(offset_value));
    }
    (sql, binds)
}
