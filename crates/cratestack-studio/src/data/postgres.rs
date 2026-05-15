//! Postgres-backed [`DataSource`] for Studio.
//!
//! The dynamic-row-to-JSON path uses Postgres's `row_to_json(t)` so we
//! don't have to decode per-column type OIDs in Rust. Each query
//! projects the model's columns into a subquery, then wraps the whole
//! thing in `row_to_json`:
//!
//! ```sql
//! SELECT row_to_json(t.*) AS row FROM (
//!   SELECT col1, col2, ...
//!   FROM "table"
//!   WHERE ($1::text IS NULL OR pk > $1::<pk-cast>)
//!   ORDER BY pk ASC
//!   LIMIT $2
//! ) t
//! ```
//!
//! Primary keys are bound as text and cast in SQL, which keeps the
//! Rust side blind to PK types.

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::Schema;
use sqlx_core::row::Row as _;
use sqlx_postgres::{PgPool, PgRow};

use super::db_errors::map_pg_error;
use super::model_info::{
    ModelSqlInfo, PkCast, find_pk_field, json_value_to_cursor, resolve_model,
};
use super::{
    ColumnSnapshot, DEFAULT_PAGE_LIMIT, DataError, DataSource, MAX_PAGE_LIMIT, Page,
    PageRequest, Row, SqlOp, SqlParam, SqlPreview,
};

#[derive(Debug, Clone)]
pub struct PostgresSource {
    pool: PgPool,
    schema: Arc<Schema>,
}

impl PostgresSource {
    pub fn new(pool: PgPool, schema: Arc<Schema>) -> Self {
        Self { pool, schema }
    }
}

pub(crate) fn build_list_sql(info: &ModelSqlInfo<'_>, limit: u32) -> String {
    let projection = info
        .columns
        .iter()
        .map(|c| format!("\"{}\"", c.column_name))
        .collect::<Vec<_>>()
        .join(", ");
    let pk = &info.pk_column;
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("($1::text IS NULL OR \"{pk}\" > $1)"),
        PkCast::BigInt => format!("($1::text IS NULL OR \"{pk}\" > $1::bigint)"),
    };
    format!(
        "SELECT row_to_json(t.*) AS row \
         FROM ( \
           SELECT {projection} \
           FROM \"{table}\" \
           WHERE {cursor_predicate} \
           ORDER BY \"{pk}\" ASC \
           LIMIT {limit} \
         ) t",
        table = info.table,
        projection = projection,
        cursor_predicate = cursor_predicate,
        pk = pk,
        limit = limit,
    )
}

pub(crate) fn build_get_sql(info: &ModelSqlInfo<'_>) -> String {
    let projection = info
        .columns
        .iter()
        .map(|c| format!("\"{}\"", c.column_name))
        .collect::<Vec<_>>()
        .join(", ");
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = $1"),
        PkCast::BigInt => format!("\"{pk}\" = $1::bigint"),
    };
    format!(
        "SELECT row_to_json(t.*) AS row \
         FROM ( \
           SELECT {projection} \
           FROM \"{table}\" \
           WHERE {pk_predicate} \
           LIMIT 1 \
         ) t",
        table = info.table,
        projection = projection,
        pk_predicate = pk_predicate,
    )
}

/// Cursor-paginated SELECT against a column with caller-supplied cast.
/// Used by the relation-follow path, which scans foreign-key columns
/// that aren't necessarily the model's primary key.
pub(crate) fn build_list_on_column_sql(
    info: &ModelSqlInfo<'_>,
    filter_column: &str,
    filter_cast: PkCast,
    limit: u32,
) -> String {
    let projection = info
        .columns
        .iter()
        .map(|c| format!("\"{}\"", c.column_name))
        .collect::<Vec<_>>()
        .join(", ");
    let pk = &info.pk_column;
    let filter_predicate = match filter_cast {
        PkCast::Text => format!("\"{filter_column}\" = $1"),
        PkCast::BigInt => format!("\"{filter_column}\" = $1::bigint"),
    };
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("($2::text IS NULL OR \"{pk}\" > $2)"),
        PkCast::BigInt => format!("($2::text IS NULL OR \"{pk}\" > $2::bigint)"),
    };
    format!(
        "SELECT row_to_json(t.*) AS row \
         FROM ( \
           SELECT {projection} \
           FROM \"{table}\" \
           WHERE {filter_predicate} AND {cursor_predicate} \
           ORDER BY \"{pk}\" ASC \
           LIMIT {limit} \
         ) t",
        table = info.table,
        projection = projection,
        filter_predicate = filter_predicate,
        cursor_predicate = cursor_predicate,
        pk = pk,
        limit = limit,
    )
}

fn clamp_limit(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT)
}

pub(crate) fn build_insert_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let names = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=columns.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let projection = info
        .columns
        .iter()
        .map(|c| format!("\"{}\"", c.column_name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "WITH inserted AS ( \
           INSERT INTO \"{table}\" ({names}) VALUES ({placeholders}) RETURNING * \
         ) \
         SELECT row_to_json(t.*) AS row FROM (SELECT {projection} FROM inserted) t",
        table = info.table,
        names = names,
        placeholders = placeholders,
        projection = projection,
    )
}

pub(crate) fn build_update_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let assignments = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let pk_placeholder = columns.len() + 1;
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ${pk_placeholder}"),
        PkCast::BigInt => format!("\"{pk}\" = ${pk_placeholder}::bigint"),
    };
    let projection = info
        .columns
        .iter()
        .map(|c| format!("\"{}\"", c.column_name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "WITH updated AS ( \
           UPDATE \"{table}\" SET {assignments} WHERE {pk_predicate} RETURNING * \
         ) \
         SELECT row_to_json(t.*) AS row FROM (SELECT {projection} FROM updated) t",
        table = info.table,
        assignments = assignments,
        pk_predicate = pk_predicate,
        projection = projection,
    )
}

pub(crate) fn build_delete_sql(info: &ModelSqlInfo<'_>) -> String {
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = $1"),
        PkCast::BigInt => format!("\"{pk}\" = $1::bigint"),
    };
    let projection = info
        .columns
        .iter()
        .map(|c| format!("\"{}\"", c.column_name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "WITH deleted AS ( \
           DELETE FROM \"{table}\" WHERE {pk_predicate} RETURNING * \
         ) \
         SELECT row_to_json(t.*) AS row FROM (SELECT {projection} FROM deleted) t",
        table = info.table,
        pk_predicate = pk_predicate,
        projection = projection,
    )
}

/// One typed value ready for a Postgres bind. Studio chooses the
/// variant from the field's declared scalar type and the incoming JSON
/// value; the variant in turn drives sqlx's encoding path. JSON / array
/// / object payloads bind through `sqlx::types::Json` so non-jsonb
/// columns get a hard error at type-check time rather than a silent
/// stringification.
#[derive(Debug, Clone)]
pub(crate) enum TypedValue {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Json(serde_json::Value),
    Null,
}

/// Walk `payload` in column order, looking up each field's scalar
/// type on the source model and producing a `TypedValue` for each
/// present key. Missing keys are skipped (the UPDATE path uses that
/// for partial updates; CREATE relies on the validator having already
/// flagged missing required keys).
pub(crate) fn collect_payload(
    schema: &cratestack_core::Schema,
    model_name: &str,
    info: &ModelSqlInfo<'_>,
    payload: &Row,
) -> (Vec<String>, Vec<TypedValue>) {
    let model = schema
        .models
        .iter()
        .find(|m| m.name == model_name)
        .expect("resolve_model already checked the model exists");
    let mut cols = Vec::new();
    let mut binds = Vec::new();
    for col in &info.columns {
        let Some(value) = payload.get(col.field_name) else {
            continue;
        };
        let field = model
            .fields
            .iter()
            .find(|f| f.name == col.field_name)
            .expect("column info was derived from the same field list");
        cols.push(col.column_name.clone());
        binds.push(json_to_typed(&field.ty.name, value));
    }
    (cols, binds)
}

fn json_to_typed(scalar: &str, value: &serde_json::Value) -> TypedValue {
    if value.is_null() {
        return TypedValue::Null;
    }
    match scalar {
        "Int" => TypedValue::Int(value.as_i64().unwrap_or_else(|| {
            value.as_f64().map(|f| f as i64).unwrap_or(0)
        })),
        "Float" => TypedValue::Float(value.as_f64().unwrap_or(0.0)),
        "Boolean" => TypedValue::Bool(value.as_bool().unwrap_or(false)),
        "Json" => TypedValue::Json(value.clone()),
        // String, Cuid, Uuid, Decimal, DateTime, Bytes, enums.
        _ => TypedValue::Text(match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }),
    }
}

/// Bind one typed value onto a sqlx Query. The `match` keeps the
/// per-variant Encode chosen at compile time even though the caller
/// sees a single function.
pub(crate) fn bind_typed<'q>(
    q: sqlx_core::query::Query<
        'q,
        sqlx_postgres::Postgres,
        sqlx_postgres::PgArguments,
    >,
    value: &TypedValue,
) -> sqlx_core::query::Query<
    'q,
    sqlx_postgres::Postgres,
    sqlx_postgres::PgArguments,
> {
    match value {
        TypedValue::Text(s) => q.bind(s.clone()),
        TypedValue::Int(i) => q.bind(*i),
        TypedValue::Float(f) => q.bind(*f),
        TypedValue::Bool(b) => q.bind(*b),
        TypedValue::Json(j) => q.bind(sqlx_core::types::Json(j.clone())),
        TypedValue::Null => q.bind(Option::<String>::None),
    }
}

#[async_trait]
impl DataSource for PostgresSource {
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError> {
        let (resolved_model, info) = resolve_model(&self.schema, model)?;
        let limit = clamp_limit(page.limit);
        let sql = build_list_sql(&info, limit);
        let pk_field_name = find_pk_field(resolved_model)
            .map(|f| f.name.clone())
            .expect("resolve_model returns an error when there is no @id");

        let rows: Vec<PgRow> = sqlx_core::query::query(&sql)
            .bind(page.cursor)
            .fetch_all(&self.pool)
            .await?;

        let decoded = decode_rows(rows)?;
        let next_cursor = if decoded.len() == limit as usize {
            decoded
                .last()
                .and_then(|r| r.get(&pk_field_name))
                .map(json_value_to_cursor)
        } else {
            None
        };

        Ok(Page {
            rows: decoded,
            next_cursor,
        })
    }

    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let sql = build_get_sql(&info);

        let row: Option<PgRow> = sqlx_core::query::query(&sql)
            .bind(pk)
            .fetch_optional(&self.pool)
            .await?;

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

    async fn create(&self, model: &str, payload: &Row) -> Result<Row, DataError> {
        let (resolved, info) = resolve_model(&self.schema, model)?;
        let (cols, binds) = collect_payload(&self.schema, model, &info, payload);
        let sql = build_insert_sql(&info, &cols);

        let mut q = sqlx_core::query::query(&sql);
        for value in &binds {
            q = bind_typed(q, value);
        }
        let row: PgRow = match q.fetch_one(&self.pool).await {
            Ok(r) => r,
            Err(e) => {
                if let Some(mapped) = map_pg_error(Some(resolved), &e) {
                    return Err(mapped);
                }
                return Err(DataError::Db(e));
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

    async fn update(
        &self,
        model: &str,
        pk: &str,
        payload: &Row,
    ) -> Result<Option<Row>, DataError> {
        let (resolved, info) = resolve_model(&self.schema, model)?;
        if payload.is_empty() {
            return self.get(model, pk).await;
        }
        let (cols, binds) = collect_payload(&self.schema, model, &info, payload);
        let sql = build_update_sql(&info, &cols);

        let mut q = sqlx_core::query::query(&sql);
        for value in &binds {
            q = bind_typed(q, value);
        }
        q = q.bind(pk);
        let row: Option<PgRow> = match q.fetch_optional(&self.pool).await {
            Ok(r) => r,
            Err(e) => {
                if let Some(mapped) = map_pg_error(Some(resolved), &e) {
                    return Err(mapped);
                }
                return Err(DataError::Db(e));
            }
        };
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

    async fn delete(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        let (resolved, info) = resolve_model(&self.schema, model)?;
        let sql = build_delete_sql(&info);
        let row: Option<PgRow> = match sqlx_core::query::query(&sql)
            .bind(pk)
            .fetch_optional(&self.pool)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                if let Some(mapped) = map_pg_error(Some(resolved), &e) {
                    return Err(mapped);
                }
                return Err(DataError::Db(e));
            }
        };
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

    async fn preview_sql(
        &self,
        op: SqlOp,
        model: &str,
        pk: Option<&str>,
        payload: Option<&Row>,
    ) -> Result<SqlPreview, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let (sql, params, notes) = match op {
            SqlOp::List => {
                let sql = build_list_sql(&info, DEFAULT_PAGE_LIMIT);
                (
                    sql,
                    vec![SqlParam {
                        index: 1,
                        binding: "cursor (NULL on first page)".to_owned(),
                        kind: "text",
                    }],
                    None,
                )
            }
            SqlOp::Get => {
                let pk_value = pk.unwrap_or("…");
                let sql = build_get_sql(&info);
                (
                    sql,
                    vec![SqlParam {
                        index: 1,
                        binding: pk_value.to_owned(),
                        kind: pk_kind(info.pk_cast),
                    }],
                    None,
                )
            }
            SqlOp::Create => {
                let (cols, binds) = payload
                    .map(|p| collect_payload(&self.schema, model, &info, p))
                    .unwrap_or_else(|| sample_columns_and_binds(&info));
                let sql = build_insert_sql(&info, &cols);
                (sql, label_params(&cols, &binds, false, info.pk_cast), None)
            }
            SqlOp::Update => {
                let (cols, binds) = payload
                    .map(|p| collect_payload(&self.schema, model, &info, p))
                    .unwrap_or_else(|| sample_columns_and_binds(&info));
                let mut params = label_params(&cols, &binds, false, info.pk_cast);
                params.push(SqlParam {
                    index: (cols.len() + 1) as u32,
                    binding: pk.unwrap_or("…").to_owned(),
                    kind: pk_kind(info.pk_cast),
                });
                let sql = build_update_sql(&info, &cols);
                (sql, params, None)
            }
            SqlOp::Delete => {
                let sql = build_delete_sql(&info);
                (
                    sql,
                    vec![SqlParam {
                        index: 1,
                        binding: pk.unwrap_or("…").to_owned(),
                        kind: pk_kind(info.pk_cast),
                    }],
                    None,
                )
            }
        };
        Ok(SqlPreview {
            driver: "postgres",
            sql,
            params,
            plan: None,
            notes,
        })
    }

    async fn inspect_columns(
        &self,
        model: &str,
    ) -> Result<Option<Vec<ColumnSnapshot>>, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let sql = "SELECT column_name, data_type, is_nullable \
                   FROM information_schema.columns \
                   WHERE table_schema = current_schema() \
                     AND table_name = $1 \
                   ORDER BY ordinal_position";
        let rows: Vec<PgRow> = sqlx_core::query::query(sql)
            .bind(info.table.clone())
            .fetch_all(&self.pool)
            .await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let snapshots = rows
            .into_iter()
            .map(|r| {
                let name: String = r.try_get(0).unwrap_or_default();
                let data_type: String = r.try_get(1).unwrap_or_default();
                let is_nullable: String = r.try_get(2).unwrap_or_default();
                ColumnSnapshot {
                    name,
                    data_type,
                    nullable: is_nullable.eq_ignore_ascii_case("YES"),
                }
            })
            .collect();
        Ok(Some(snapshots))
    }

    async fn follow(
        &self,
        target_model: &str,
        filter_column: &str,
        filter_cast: PkCast,
        filter_value: &str,
        page: PageRequest<'_>,
    ) -> Result<Page, DataError> {
        let (resolved_model, info) = resolve_model(&self.schema, target_model)?;
        let limit = clamp_limit(page.limit);
        let sql = build_list_on_column_sql(&info, filter_column, filter_cast, limit);
        let pk_field_name = find_pk_field(resolved_model)
            .map(|f| f.name.clone())
            .expect("resolve_model returns an error when there is no @id");

        let rows: Vec<PgRow> = sqlx_core::query::query(&sql)
            .bind(filter_value)
            .bind(page.cursor)
            .fetch_all(&self.pool)
            .await?;

        let decoded = decode_rows(rows)?;
        let next_cursor = if decoded.len() == limit as usize {
            decoded
                .last()
                .and_then(|r| r.get(&pk_field_name))
                .map(json_value_to_cursor)
        } else {
            None
        };

        Ok(Page {
            rows: decoded,
            next_cursor,
        })
    }
}

pub(crate) fn pk_kind(cast: PkCast) -> &'static str {
    match cast {
        PkCast::Text => "text",
        PkCast::BigInt => "bigint",
    }
}

/// Synthesize one bind per scalar column so callers can preview a
/// CREATE / UPDATE without first crafting a payload. The labels are
/// placeholders meant for the SQL preview UI, not for execution.
fn sample_columns_and_binds(
    info: &ModelSqlInfo<'_>,
) -> (Vec<String>, Vec<TypedValue>) {
    let cols = info.columns.iter().map(|c| c.column_name.clone()).collect();
    let binds = info
        .columns
        .iter()
        .map(|_| TypedValue::Text("…".to_owned()))
        .collect();
    (cols, binds)
}

fn label_params(
    cols: &[String],
    binds: &[TypedValue],
    _partial: bool,
    _pk: PkCast,
) -> Vec<SqlParam> {
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

fn typed_kind(value: &TypedValue) -> &'static str {
    match value {
        TypedValue::Text(_) => "text",
        TypedValue::Int(_) => "bigint",
        TypedValue::Float(_) => "double",
        TypedValue::Bool(_) => "boolean",
        TypedValue::Json(_) => "jsonb",
        TypedValue::Null => "null",
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(schema_text: &str) -> Schema {
        cratestack_parser::parse_schema(schema_text).expect("schema parses")
    }

    #[test]
    fn list_sql_uses_text_cursor_predicate_for_string_pk() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                }
            "#,
        );
        let (_, info) = resolve_model(&schema, "Post").unwrap();
        let sql = build_list_sql(&info, 50);
        assert!(sql.contains(r#""id" > $1"#), "{sql}");
        assert!(!sql.contains("::bigint"), "{sql}");
        assert!(sql.contains("LIMIT 50"), "{sql}");
        assert!(sql.contains(r#"FROM "posts""#), "{sql}");
    }

    #[test]
    fn list_sql_casts_to_bigint_for_int_pk() {
        let schema = parse(
            r#"
                model Customer {
                  id Int @id
                  email String
                }
            "#,
        );
        let (_, info) = resolve_model(&schema, "Customer").unwrap();
        let sql = build_list_sql(&info, 10);
        assert_eq!(info.pk_cast, PkCast::BigInt);
        assert!(sql.contains(r#""id" > $1::bigint"#), "{sql}");
        assert!(sql.contains("LIMIT 10"), "{sql}");
    }

    #[test]
    fn get_sql_uses_bigint_cast_for_int_pk() {
        let schema = parse(
            r#"
                model Customer {
                  id Int @id
                  email String
                }
            "#,
        );
        let (_, info) = resolve_model(&schema, "Customer").unwrap();
        let sql = build_get_sql(&info);
        assert!(sql.contains(r#""id" = $1::bigint"#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn list_on_column_filters_and_pages_simultaneously() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  authorId String
                  title String
                }
            "#,
        );
        let (_, info) = resolve_model(&schema, "Post").unwrap();
        let sql = build_list_on_column_sql(&info, "author_id", PkCast::Text, 25);
        assert!(sql.contains(r#""author_id" = $1"#), "{sql}");
        assert!(sql.contains(r#""id" > $2"#), "{sql}");
        assert!(sql.contains("LIMIT 25"), "{sql}");
    }
}
