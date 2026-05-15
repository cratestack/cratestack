//! `preview_sql` rendering for the Postgres source.

use crate::data::common::sample_column_names;
use crate::data::model_info::{ModelSqlInfo, PkCast};
use crate::data::{DEFAULT_PAGE_LIMIT, Row, SqlOp, SqlParam, SqlPreview};

use super::bindings::{TypedValue, collect_payload, typed_kind};
use super::sql::{
    build_delete_sql, build_get_sql, build_insert_sql, build_list_sql, build_update_sql,
};

/// Render the preview for one operation. `model_name` is needed by the
/// payload collector to look up declared scalar types on the schema.
pub(super) fn render(
    schema: &cratestack_core::Schema,
    info: &ModelSqlInfo<'_>,
    model_name: &str,
    op: SqlOp,
    pk: Option<&str>,
    payload: Option<&Row>,
) -> SqlPreview {
    let (sql, params) = match op {
        SqlOp::List => (
            build_list_sql(info, DEFAULT_PAGE_LIMIT),
            vec![SqlParam {
                index: 1,
                binding: "cursor (NULL on first page)".to_owned(),
                kind: "text",
            }],
        ),
        SqlOp::Get => (
            build_get_sql(info),
            vec![pk_param(1, pk, info.pk_cast)],
        ),
        SqlOp::Create => {
            let (cols, binds) = payload
                .map(|p| collect_payload(schema, model_name, info, p))
                .unwrap_or_else(|| sample_columns_and_binds(info));
            (build_insert_sql(info, &cols), label_params(&cols, &binds))
        }
        SqlOp::Update => {
            let (cols, binds) = payload
                .map(|p| collect_payload(schema, model_name, info, p))
                .unwrap_or_else(|| sample_columns_and_binds(info));
            let mut params = label_params(&cols, &binds);
            params.push(pk_param((cols.len() + 1) as u32, pk, info.pk_cast));
            (build_update_sql(info, &cols), params)
        }
        SqlOp::Delete => (
            build_delete_sql(info),
            vec![pk_param(1, pk, info.pk_cast)],
        ),
    };
    SqlPreview {
        driver: "postgres",
        sql,
        params,
        plan: None,
        notes: None,
    }
}

fn pk_param(index: u32, pk: Option<&str>, cast: PkCast) -> SqlParam {
    SqlParam {
        index,
        binding: pk.unwrap_or("…").to_owned(),
        kind: pk_kind(cast),
    }
}

pub(crate) fn pk_kind(cast: PkCast) -> &'static str {
    match cast {
        PkCast::Text => "text",
        PkCast::BigInt => "bigint",
    }
}

fn sample_columns_and_binds(info: &ModelSqlInfo<'_>) -> (Vec<String>, Vec<TypedValue>) {
    let cols = sample_column_names(info);
    let binds = info
        .columns
        .iter()
        .map(|_| TypedValue::Text("…".to_owned()))
        .collect();
    (cols, binds)
}

fn label_params(cols: &[String], binds: &[TypedValue]) -> Vec<SqlParam> {
    cols.iter()
        .zip(binds.iter())
        .enumerate()
        .map(|(i, (col, bind))| SqlParam {
            index: (i + 1) as u32,
            binding: col.clone(),
            kind: typed_kind(bind),
        })
        .collect()
}
