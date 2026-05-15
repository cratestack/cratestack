//! Execution-side operations for the Postgres source.
//!
//! Each function composes a SQL builder ([`super::sql`]) and the
//! typed-bind helpers ([`super::bindings`]) into one async CRUD action
//! against an `&PgPool`. Constraint failures route through
//! [`crate::data::db_errors::map_pg_error`] so the UI can display
//! per-field errors.

use cratestack_core::Schema;
use sqlx_core::row::Row as _;
use sqlx_postgres::{PgPool, PgRow};

use crate::data::common::{clamp_limit, next_cursor};
use crate::data::db_errors::map_pg_error;
use crate::data::model_info::{PkCast, find_pk_field, resolve_model};
use crate::data::{DataError, Page, PageRequest, Row};

use super::bindings::{bind_typed, collect_payload};
use super::sql::{
    build_delete_sql, build_get_sql, build_insert_sql, build_list_on_column_sql, build_list_sql,
    build_update_sql,
};

pub(super) async fn list(
    schema: &Schema,
    pool: &PgPool,
    model: &str,
    page: PageRequest<'_>,
) -> Result<Page, DataError> {
    let (resolved_model, info) = resolve_model(schema, model)?;
    let limit = clamp_limit(page.limit);
    let sql = build_list_sql(&info, limit);
    let pk_field_name = find_pk_field(resolved_model)
        .map(|f| f.name.clone())
        .expect("resolve_model returns an error when there is no @id");

    let rows: Vec<PgRow> = sqlx_core::query::query(&sql)
        .bind(page.cursor)
        .fetch_all(pool)
        .await?;

    let decoded = decode_rows(rows)?;
    let next_cursor = next_cursor(&decoded, &pk_field_name, limit);
    Ok(Page {
        rows: decoded,
        next_cursor,
    })
}

pub(super) async fn get(
    schema: &Schema,
    pool: &PgPool,
    model: &str,
    pk: &str,
) -> Result<Option<Row>, DataError> {
    let (_, info) = resolve_model(schema, model)?;
    let sql = build_get_sql(&info);

    let row: Option<PgRow> = sqlx_core::query::query(&sql)
        .bind(pk)
        .fetch_optional(pool)
        .await?;

    decode_optional(row)
}

pub(super) async fn create(
    schema: &Schema,
    pool: &PgPool,
    model: &str,
    payload: &Row,
) -> Result<Row, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let (cols, binds) = collect_payload(schema, model, &info, payload);
    let sql = build_insert_sql(&info, &cols);

    let mut q = sqlx_core::query::query(&sql);
    for value in &binds {
        q = bind_typed(q, value);
    }
    let row = match q.fetch_one(pool).await {
        Ok(r) => r,
        Err(e) => {
            return Err(map_pg_error(Some(resolved), &e).unwrap_or(DataError::Db(e)));
        }
    };
    let value: serde_json::Value = row.try_get(0)?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(DataError::Unsupported {
            what: "INSERT … RETURNING did not produce a JSON object",
        }),
    }
}

pub(super) async fn update(
    schema: &Schema,
    pool: &PgPool,
    model: &str,
    pk: &str,
    payload: &Row,
) -> Result<Option<Row>, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let (cols, binds) = collect_payload(schema, model, &info, payload);
    let sql = build_update_sql(&info, &cols);

    let mut q = sqlx_core::query::query(&sql);
    for value in &binds {
        q = bind_typed(q, value);
    }
    q = q.bind(pk);
    let row = match q.fetch_optional(pool).await {
        Ok(r) => r,
        Err(e) => {
            return Err(map_pg_error(Some(resolved), &e).unwrap_or(DataError::Db(e)));
        }
    };
    decode_optional(row)
}

pub(super) async fn delete(
    schema: &Schema,
    pool: &PgPool,
    model: &str,
    pk: &str,
) -> Result<Option<Row>, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let sql = build_delete_sql(&info);
    let row = match sqlx_core::query::query(&sql)
        .bind(pk)
        .fetch_optional(pool)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Err(map_pg_error(Some(resolved), &e).unwrap_or(DataError::Db(e)));
        }
    };
    decode_optional(row)
}

pub(super) async fn follow(
    schema: &Schema,
    pool: &PgPool,
    target_model: &str,
    filter_column: &str,
    filter_cast: PkCast,
    filter_value: &str,
    page: PageRequest<'_>,
) -> Result<Page, DataError> {
    let (resolved_model, info) = resolve_model(schema, target_model)?;
    let limit = clamp_limit(page.limit);
    let sql = build_list_on_column_sql(&info, filter_column, filter_cast, limit);
    let pk_field_name = find_pk_field(resolved_model)
        .map(|f| f.name.clone())
        .expect("resolve_model returns an error when there is no @id");

    let rows: Vec<PgRow> = sqlx_core::query::query(&sql)
        .bind(filter_value)
        .bind(page.cursor)
        .fetch_all(pool)
        .await?;

    let decoded = decode_rows(rows)?;
    let next_cursor = next_cursor(&decoded, &pk_field_name, limit);
    Ok(Page {
        rows: decoded,
        next_cursor,
    })
}

fn decode_rows(rows: Vec<PgRow>) -> Result<Vec<Row>, DataError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let value: serde_json::Value = row.try_get(0)?;
        if let serde_json::Value::Object(map) = value {
            out.push(map);
        }
    }
    Ok(out)
}

fn decode_optional(row: Option<PgRow>) -> Result<Option<Row>, DataError> {
    match row {
        None => Ok(None),
        Some(r) => {
            let value: serde_json::Value = r.try_get(0)?;
            Ok(match value {
                serde_json::Value::Object(map) => Some(map),
                _ => None,
            })
        }
    }
}
