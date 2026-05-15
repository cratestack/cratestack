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
//! Rust side blind to PK types. Phase 1a supports String- and
//! Int/BigInt-typed `@id` fields; other PK types fall through to
//! [`DataError::Unsupported`].

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::{Field, Model, Schema};
use cratestack_migrate::{column_name, table_name};
use sqlx_postgres::{PgPool, PgRow};
use sqlx_core::row::Row as _;

use super::{
    DEFAULT_PAGE_LIMIT, DataError, DataSource, MAX_PAGE_LIMIT, Page, PageRequest, Row,
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

/// Project the columns and primary-key info needed to build a SELECT
/// for one model. Pulled out so we can unit-test SQL generation
/// without a live database.
#[derive(Debug)]
pub(crate) struct ModelSqlInfo<'a> {
    pub table: String,
    pub columns: Vec<ColumnInfo<'a>>,
    pub pk_column: String,
    pub pk_cast: PkCast,
}

#[derive(Debug)]
pub(crate) struct ColumnInfo<'a> {
    /// The field's `.cstack` name (camelCase). The snippet generator
    /// will use this in Phase 1b once the snippet projects fields; for
    /// now it's read only by tests.
    #[allow(dead_code)]
    pub field_name: &'a str,
    pub column_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkCast {
    /// `WHERE pk = $1` — bind as text directly, no SQL cast.
    Text,
    /// `WHERE pk = $1::bigint`.
    BigInt,
}

pub(crate) fn resolve_model<'a>(
    schema: &'a Schema,
    model_name: &str,
) -> Result<(&'a Model, ModelSqlInfo<'a>), DataError> {
    let model = schema
        .models
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| DataError::UnknownModel {
            model: model_name.to_owned(),
        })?;

    let pk_field = find_pk_field(model).ok_or_else(|| DataError::NoPrimaryKey {
        model: model_name.to_owned(),
    })?;

    let columns = model
        .fields
        .iter()
        .filter(|f| is_scalar_field(schema, f))
        .map(|f| ColumnInfo {
            field_name: f.name.as_str(),
            column_name: column_name(&f.name),
        })
        .collect();

    let pk_cast = pk_cast_for(&pk_field.ty.name).ok_or_else(|| DataError::Unsupported {
        what: "primary key of this type (Phase 1a supports String, Int, BigInt)",
    })?;

    Ok((
        model,
        ModelSqlInfo {
            table: table_name(&model.name),
            columns,
            pk_column: column_name(&pk_field.name),
            pk_cast,
        },
    ))
}

fn find_pk_field(model: &Model) -> Option<&Field> {
    model
        .fields
        .iter()
        .find(|f| f.attributes.iter().any(|a| a.raw.starts_with("@id")))
}

/// Phase 1a treats relation-shaped fields as non-projectable. A field
/// counts as scalar if its arity isn't `List` and its declared type
/// doesn't name a model in the same schema. Enum-typed fields stay
/// scalar (they're stored as text columns).
fn is_scalar_field(schema: &Schema, field: &Field) -> bool {
    if matches!(field.ty.arity, cratestack_core::TypeArity::List) {
        return false;
    }
    !schema.models.iter().any(|m| m.name == field.ty.name)
}

fn pk_cast_for(scalar: &str) -> Option<PkCast> {
    match scalar {
        "String" | "Uuid" | "Cuid" | "Decimal" => Some(PkCast::Text),
        "Int" => Some(PkCast::BigInt),
        _ => None,
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

fn clamp_limit(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT)
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

        let mut decoded: Vec<Row> = Vec::with_capacity(rows.len());
        for row in rows {
            let value: serde_json::Value = row.try_get(0)?;
            if let serde_json::Value::Object(map) = value {
                decoded.push(map);
            }
        }

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
}

/// JSON values come back as strings or numbers depending on the PK
/// column type; both serialize losslessly as a `String` cursor that we
/// later re-bind as `text` and cast in SQL.
fn json_value_to_cursor(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(schema_text: &str) -> Schema {
        cratestack_parser::parse_schema(schema_text).expect("schema parses")
    }

    #[test]
    fn resolves_model_with_string_pk() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                }
            "#,
        );
        let (_, info) = resolve_model(&schema, "Post").expect("resolves");
        assert_eq!(info.table, "posts");
        assert_eq!(info.pk_column, "id");
        assert_eq!(info.pk_cast, PkCast::Text);
        let columns: Vec<&str> = info.columns.iter().map(|c| c.field_name).collect();
        assert_eq!(columns, vec!["id", "title"]);
    }

    #[test]
    fn resolves_model_with_int_pk() {
        let schema = parse(
            r#"
                model Customer {
                  id Int @id
                  email String
                }
            "#,
        );
        let (_, info) = resolve_model(&schema, "Customer").expect("resolves");
        assert_eq!(info.pk_cast, PkCast::BigInt);
    }

    #[test]
    fn unknown_model_errors() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                }
            "#,
        );
        let error = resolve_model(&schema, "Nope").expect_err("missing model errors");
        assert!(matches!(error, DataError::UnknownModel { ref model } if model == "Nope"));
    }

    #[test]
    fn unsupported_pk_type_errors() {
        let schema = parse(
            r#"
                model Event {
                  id DateTime @id
                  label String
                }
            "#,
        );
        let error = resolve_model(&schema, "Event").expect_err("unsupported pk fails");
        assert!(matches!(error, DataError::Unsupported { what } if what.contains("primary key")));
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
    fn skips_list_arity_fields_from_projection() {
        let schema = parse(
            r#"
                model Author {
                  id String @id
                  name String
                  tags String[]
                }
            "#,
        );
        let (_, info) = resolve_model(&schema, "Author").unwrap();
        let fields: Vec<&str> = info.columns.iter().map(|c| c.field_name).collect();
        assert_eq!(fields, vec!["id", "name"]);
    }
}
