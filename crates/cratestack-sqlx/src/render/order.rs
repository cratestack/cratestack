//! `ORDER BY` clause SQL rendering — scalar columns + relation-scalar
//! `(SELECT ...)` subselects, with direction + null-ordering suffixes.

use std::fmt::Write;

use cratestack_core::CratestackContext;
use cratestack_sql::OrderTarget;

use crate::{OrderClause, SortDirection};

use super::relation::render_relation_value_sql;

/// `ctx`: see [`super::filter::render_filter_sql`] — `None` renders relation
/// sort subqueries without their related read scope.
pub(crate) fn render_order_clause_sql(
    clause: &OrderClause,
    sql: &mut String,
    bind_index: &mut usize,
    ctx: Option<&CratestackContext>,
) {
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
        OrderTarget::RelationScalar { hops, column } => {
            render_relation_value_sql(hops, column, sql, bind_index, ctx);
            let _ = write!(
                sql,
                " {} {}",
                sort_direction_sql(clause.direction),
                null_order_sql(clause.null_order),
            );
        }
        OrderTarget::VectorDistance { column, metric, .. } => {
            // Only the query vector binds a placeholder — the column
            // and operator are static SQL text, matching how
            // `render_spatial_filter_sql` treats its column names.
            let _ = write!(
                sql,
                "({} {} ${}) {} {}",
                column,
                metric.sql_operator(),
                *bind_index,
                sort_direction_sql(clause.direction),
                null_order_sql(clause.null_order),
            );
            *bind_index += 1;
        }
        #[cfg(feature = "postgis")]
        OrderTarget::SpatialDistance { column, .. } => {
            // `ST_Distance(col::geography, ST_MakePoint($lng, $lat)::geography)`
            // — two bind slots, lng then lat, matching the argument
            // order `render_spatial_filter_sql` uses for the
            // `Covers`/`DWithin` filters so the bind sequence stays
            // consistent across the spatial surface.
            let _ = write!(
                sql,
                "ST_Distance({column}::geography, ST_MakePoint(${lng}, ${lat})::geography) {dir} \
                 {nulls}",
                lng = *bind_index,
                lat = *bind_index + 1,
                dir = sort_direction_sql(clause.direction),
                nulls = null_order_sql(clause.null_order),
            );
            *bind_index += 2;
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
