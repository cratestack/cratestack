//! `preview_sql` + `preview_scoped_sql` for `FindMany`. The "preview"
//! string-builders never bind values — they exist for the studio's
//! "show me the SQL that'll run" pane.

use cratestack_core::CoolContext;

use crate::render::{render_filter_expr_sql, render_order_clause_sql, render_scoped_select_sql};

use super::find_many::FindMany;

pub(super) fn preview_sql<M, PK>(find: &FindMany<'_, M, PK>) -> String {
    let mut sql = format!(
        "SELECT {} FROM {}",
        find.descriptor.select_projection(),
        find.descriptor.table_name(),
    );
    let order_by = find.effective_order_by();

    let mut bind_index = 1usize;
    if !find.filters.is_empty() {
        sql.push_str(" WHERE ");
        for (index, filter) in find.filters.iter().enumerate() {
            if index > 0 {
                sql.push_str(" AND ");
            }
            render_filter_expr_sql(filter, &mut sql, &mut bind_index);
        }
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

    match (find.limit, find.offset) {
        (Some(_), Some(_)) => {
            sql.push_str(&format!(" LIMIT ${bind_index} OFFSET ${}", bind_index + 1));
        }
        (Some(_), None) => {
            sql.push_str(&format!(" LIMIT ${bind_index}"));
        }
        (None, Some(_)) => {
            sql.push_str(&format!(" OFFSET ${bind_index}"));
        }
        (None, None) => {}
    }

    if find.for_update {
        sql.push_str(" FOR UPDATE");
    }

    sql
}

pub(super) fn preview_scoped_sql<M, PK>(find: &FindMany<'_, M, PK>, ctx: &CoolContext) -> String {
    let order_by = find.effective_order_by();
    render_scoped_select_sql(
        find.descriptor,
        &find.filters,
        &order_by,
        find.limit,
        find.offset,
        ctx,
    )
}
