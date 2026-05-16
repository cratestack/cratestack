//! Per-kind filter rendering for spatial / JSON / coalesce filters —
//! each is a distinct SQL shape (PostGIS function call, jsonb operator
//! chain, COALESCE() comparison) and lives in its own helper to keep
//! the top-level filter dispatcher readable.

use std::fmt::Write;

use cratestack_sql::FilterOp;

pub(super) fn render_spatial_filter_sql(
    filter: &cratestack_sql::SpatialFilter,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match filter {
        cratestack_sql::SpatialFilter::CoversGeographyPoint { column, .. } => {
            let _ = write!(
                sql,
                "ST_Covers({column}::geography, ST_MakePoint(${lng}, ${lat})::geography)",
                lng = *bind_index,
                lat = *bind_index + 1,
            );
            *bind_index += 2;
        }
        cratestack_sql::SpatialFilter::DWithinGeographyPoint { column, .. } => {
            let _ = write!(
                sql,
                "ST_DWithin({column}::geography, ST_MakePoint(${lng}, ${lat})::geography, ${rad})",
                lng = *bind_index,
                lat = *bind_index + 1,
                rad = *bind_index + 2,
            );
            *bind_index += 3;
        }
    }
}

pub(super) fn render_json_filter_sql(
    filter: &cratestack_sql::JsonFilter,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match filter {
        cratestack_sql::JsonFilter::HasKey { column, key: _ } => {
            let _ = write!(sql, "{column} ? ${bind_index}");
            *bind_index += 1;
        }
        cratestack_sql::JsonFilter::GetText {
            column,
            key: _,
            op,
            value: _,
        } => {
            let _ = write!(sql, "{column} ->> ${bind_index}");
            *bind_index += 1;
            match op {
                FilterOp::Eq => render_json_text_binary_sql("=", sql, bind_index),
                FilterOp::Ne => render_json_text_binary_sql("!=", sql, bind_index),
                FilterOp::Lt => render_json_text_binary_sql("<", sql, bind_index),
                FilterOp::Lte => render_json_text_binary_sql("<=", sql, bind_index),
                FilterOp::Gt => render_json_text_binary_sql(">", sql, bind_index),
                FilterOp::Gte => render_json_text_binary_sql(">=", sql, bind_index),
                FilterOp::IsNull => sql.push_str(" IS NULL"),
                FilterOp::IsNotNull => sql.push_str(" IS NOT NULL"),
                FilterOp::In
                | FilterOp::Contains
                | FilterOp::StartsWith
                | FilterOp::EqOrNull => {
                    unreachable!("JsonFilter::GetText built with unsupported op {:?}", op);
                }
            }
        }
    }
}

fn render_json_text_binary_sql(operator: &str, sql: &mut String, bind_index: &mut usize) {
    let _ = write!(sql, " {operator} ${bind_index}");
    *bind_index += 1;
}

pub(super) fn render_coalesce_filter_sql(
    filter: &cratestack_sql::CoalesceFilter,
    sql: &mut String,
    bind_index: &mut usize,
) {
    sql.push_str("COALESCE(");
    for (idx, column) in filter.columns.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(column);
    }
    sql.push(')');
    match filter.op {
        FilterOp::Eq => render_coalesce_binary_sql("=", sql, bind_index),
        FilterOp::Ne => render_coalesce_binary_sql("!=", sql, bind_index),
        FilterOp::Lt => render_coalesce_binary_sql("<", sql, bind_index),
        FilterOp::Lte => render_coalesce_binary_sql("<=", sql, bind_index),
        FilterOp::Gt => render_coalesce_binary_sql(">", sql, bind_index),
        FilterOp::Gte => render_coalesce_binary_sql(">=", sql, bind_index),
        FilterOp::IsNull => sql.push_str(" IS NULL"),
        FilterOp::IsNotNull => sql.push_str(" IS NOT NULL"),
        FilterOp::In | FilterOp::Contains | FilterOp::StartsWith | FilterOp::EqOrNull => {
            unreachable!("CoalesceFilter built with unsupported op {:?}", filter.op);
        }
    }
}

fn render_coalesce_binary_sql(operator: &str, sql: &mut String, bind_index: &mut usize) {
    let _ = write!(sql, " {operator} ${bind_index}");
    *bind_index += 1;
}
