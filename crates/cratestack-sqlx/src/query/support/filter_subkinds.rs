//! Per-kind filter pushers for spatial / JSON / coalesce filters.
//! Each is a distinct SQL shape and lives in its own helper.

use cratestack_sql::{FilterOp, FilterValue};

use crate::sqlx;

use super::values::push_bind_value;

pub(super) fn push_spatial_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filter: &cratestack_sql::SpatialFilter,
) {
    match filter {
        cratestack_sql::SpatialFilter::CoversGeographyPoint { column, lng, lat } => {
            query
                .push("ST_Covers(")
                .push(*column)
                .push("::geography, ST_MakePoint(");
            query.push_bind(*lng);
            query.push(", ");
            query.push_bind(*lat);
            query.push(")::geography)");
        }
        cratestack_sql::SpatialFilter::DWithinGeographyPoint {
            column,
            lng,
            lat,
            radius_meters,
        } => {
            query
                .push("ST_DWithin(")
                .push(*column)
                .push("::geography, ST_MakePoint(");
            query.push_bind(*lng);
            query.push(", ");
            query.push_bind(*lat);
            query.push(")::geography, ");
            query.push_bind(*radius_meters);
            query.push(")");
        }
    }
}

pub(super) fn push_json_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filter: &cratestack_sql::JsonFilter,
) {
    match filter {
        cratestack_sql::JsonFilter::HasKey { column, key } => {
            query.push(*column).push(" ? ");
            query.push_bind((*key).to_owned());
        }
        cratestack_sql::JsonFilter::GetText {
            column,
            key,
            op,
            value,
        } => {
            query.push(*column).push(" ->> ");
            query.push_bind((*key).to_owned());
            match op {
                FilterOp::Eq => push_json_get_text_binary(query, "=", value),
                FilterOp::Ne => push_json_get_text_binary(query, "!=", value),
                FilterOp::Lt => push_json_get_text_binary(query, "<", value),
                FilterOp::Lte => push_json_get_text_binary(query, "<=", value),
                FilterOp::Gt => push_json_get_text_binary(query, ">", value),
                FilterOp::Gte => push_json_get_text_binary(query, ">=", value),
                FilterOp::IsNull => {
                    query.push(" IS NULL");
                }
                FilterOp::IsNotNull => {
                    query.push(" IS NOT NULL");
                }
                FilterOp::In | FilterOp::Contains | FilterOp::StartsWith | FilterOp::EqOrNull => {
                    unreachable!("JsonFilter::GetText built with unsupported op {:?}", op);
                }
            }
        }
    }
}

fn push_json_get_text_binary(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    operator: &str,
    value: &FilterValue,
) {
    query.push(" ").push(operator).push(" ");
    let FilterValue::Single(value) = value else {
        unreachable!("json_get_text comparison requires FilterValue::Single");
    };
    push_bind_value(query, value);
}

pub(super) fn push_coalesce_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filter: &cratestack_sql::CoalesceFilter,
) {
    query.push("COALESCE(");
    for (idx, column) in filter.columns.iter().enumerate() {
        if idx > 0 {
            query.push(", ");
        }
        query.push(*column);
    }
    query.push(")");
    match filter.op {
        FilterOp::Eq => push_coalesce_binary(query, "=", &filter.value),
        FilterOp::Ne => push_coalesce_binary(query, "!=", &filter.value),
        FilterOp::Lt => push_coalesce_binary(query, "<", &filter.value),
        FilterOp::Lte => push_coalesce_binary(query, "<=", &filter.value),
        FilterOp::Gt => push_coalesce_binary(query, ">", &filter.value),
        FilterOp::Gte => push_coalesce_binary(query, ">=", &filter.value),
        FilterOp::IsNull => {
            query.push(" IS NULL");
        }
        FilterOp::IsNotNull => {
            query.push(" IS NOT NULL");
        }
        // IN/LIKE against a coalesced tuple invites footguns; EqOrNull
        // has no LHS column to null-check. Fail loud at construction.
        FilterOp::In | FilterOp::Contains | FilterOp::StartsWith | FilterOp::EqOrNull => {
            unreachable!(
                "CoalesceFilter built with unsupported op {:?}; only Eq/Ne/Lt/Lte/Gt/Gte/IsNull/IsNotNull are valid",
                filter.op,
            );
        }
    }
}

fn push_coalesce_binary(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    operator: &str,
    value: &FilterValue,
) {
    query.push(" ").push(operator).push(" ");
    let FilterValue::Single(value) = value else {
        unreachable!("coalesce comparison requires FilterValue::Single");
    };
    push_bind_value(query, value);
}
