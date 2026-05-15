//! SQLite-backed [`DataSource`] for Studio.
//!
//! Studio uses `rusqlite` rather than `sqlx-sqlite` because the wider
//! workspace pins `rusqlite 0.39 → libsqlite3-sys 0.37` (via
//! `cratestack-rusqlite` and `cratestack-client-store-sqlite`), and
//! Cargo's `links = "sqlite3"` rule forbids a second
//! `libsqlite3-sys` version in the graph.
//!
//! rusqlite is synchronous; every query runs through
//! [`tokio::task::spawn_blocking`] with a connection held behind a
//! [`tokio::sync::Mutex`]. SQLite is single-writer anyway, so the
//! per-source serialization isn't a scaling loss in practice.
//!
//! Row projection uses SQLite's `json_object()` builtin (available in
//! the bundled SQLite our `rusqlite` pulls in), which mirrors
//! Postgres's `row_to_json` so the rest of the pipeline can stay
//! identical:
//!
//! ```sql
//! SELECT json_object('field1', col1, 'field2', col2, …) AS row
//! FROM "table" WHERE … ORDER BY pk LIMIT ?
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::Schema;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::db_errors::map_sqlite_error;
use super::model_info::{
    ColumnInfo, ModelSqlInfo, PkCast, find_pk_field, json_value_to_cursor, resolve_model,
};
use super::{
    ColumnSnapshot, DEFAULT_PAGE_LIMIT, DataError, DataSource, MAX_PAGE_LIMIT, Page,
    PageRequest, Row, SqlOp, SqlParam, SqlPreview,
};

#[derive(Debug)]
pub struct SqliteSource {
    /// `rusqlite::Connection` isn't `Send` on its own but is when
    /// wrapped behind a mutex and accessed only from spawn_blocking.
    /// We keep one connection per source — fine for Studio's
    /// expected load (a single developer browsing).
    connection: Arc<Mutex<Connection>>,
    schema: Arc<Schema>,
}

impl SqliteSource {
    pub fn new(connection: Connection, schema: Arc<Schema>) -> Self {
        Self {
            connection: Arc::new(Mutex::new(connection)),
            schema,
        }
    }
}

pub(crate) fn build_json_object(info: &ModelSqlInfo<'_>) -> String {
    info.columns
        .iter()
        .map(|c| {
            format!(
                "'{name}', \"{col}\"",
                name = sql_quote_string(c.field_name),
                col = c.column_name
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn build_list_sql(info: &ModelSqlInfo<'_>, limit: u32) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("(?1 IS NULL OR \"{pk}\" > ?1)"),
        PkCast::BigInt => format!("(?1 IS NULL OR \"{pk}\" > CAST(?1 AS INTEGER))"),
    };
    format!(
        "SELECT json_object({object}) AS row \
         FROM \"{table}\" \
         WHERE {cursor_predicate} \
         ORDER BY \"{pk}\" ASC \
         LIMIT {limit}",
        table = info.table,
        object = object,
        cursor_predicate = cursor_predicate,
        pk = pk,
        limit = limit,
    )
}

pub(crate) fn build_get_sql(info: &ModelSqlInfo<'_>) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ?1"),
        PkCast::BigInt => format!("\"{pk}\" = CAST(?1 AS INTEGER)"),
    };
    format!(
        "SELECT json_object({object}) AS row \
         FROM \"{table}\" \
         WHERE {pk_predicate} \
         LIMIT 1",
        table = info.table,
        object = object,
        pk_predicate = pk_predicate,
    )
}

pub(crate) fn build_list_on_column_sql(
    info: &ModelSqlInfo<'_>,
    filter_column: &str,
    filter_cast: PkCast,
    limit: u32,
) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let filter_predicate = match filter_cast {
        PkCast::Text => format!("\"{filter_column}\" = ?1"),
        PkCast::BigInt => format!("\"{filter_column}\" = CAST(?1 AS INTEGER)"),
    };
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("(?2 IS NULL OR \"{pk}\" > ?2)"),
        PkCast::BigInt => format!("(?2 IS NULL OR \"{pk}\" > CAST(?2 AS INTEGER))"),
    };
    format!(
        "SELECT json_object({object}) AS row \
         FROM \"{table}\" \
         WHERE {filter_predicate} AND {cursor_predicate} \
         ORDER BY \"{pk}\" ASC \
         LIMIT {limit}",
        table = info.table,
        object = object,
        filter_predicate = filter_predicate,
        cursor_predicate = cursor_predicate,
        pk = pk,
        limit = limit,
    )
}

/// SQLite single-quote escape for use inside a literal.
fn sql_quote_string(value: &str) -> String {
    value.replace('\'', "''")
}

pub(crate) fn build_insert_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let object = build_json_object(info);
    let names = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=columns.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{table}\" ({names}) VALUES ({placeholders}) \
         RETURNING json_object({object}) AS row",
        table = info.table,
        object = object,
        names = names,
        placeholders = placeholders,
    )
}

pub(crate) fn build_update_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let object = build_json_object(info);
    let assignments = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let pk_placeholder = columns.len() + 1;
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ?{pk_placeholder}"),
        PkCast::BigInt => {
            format!("\"{pk}\" = CAST(?{pk_placeholder} AS INTEGER)")
        }
    };
    format!(
        "UPDATE \"{table}\" SET {assignments} WHERE {pk_predicate} \
         RETURNING json_object({object}) AS row",
        table = info.table,
        object = object,
        assignments = assignments,
        pk_predicate = pk_predicate,
    )
}

pub(crate) fn build_delete_sql(info: &ModelSqlInfo<'_>) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ?1"),
        PkCast::BigInt => format!("\"{pk}\" = CAST(?1 AS INTEGER)"),
    };
    format!(
        "DELETE FROM \"{table}\" WHERE {pk_predicate} \
         RETURNING json_object({object}) AS row",
        table = info.table,
        object = object,
        pk_predicate = pk_predicate,
    )
}

/// Map the payload object to `(columns, bound_values)` where each
/// value is a `rusqlite::types::Value` ready to bind. `partial = true`
/// (update) only includes keys present in the payload; `partial = false`
/// (create) emits every payload key in a deterministic order.
pub(crate) fn build_payload_bindings(
    info: &ModelSqlInfo<'_>,
    payload: &Row,
    _partial: bool,
) -> (Vec<String>, Vec<rusqlite::types::Value>) {
    let mut columns = Vec::new();
    let mut values = Vec::new();
    for col in &info.columns {
        let Some(json_value) = payload.get(col.field_name) else {
            continue;
        };
        columns.push(col.column_name.clone());
        values.push(json_to_sqlite(json_value));
    }
    (columns, values)
}

fn json_to_sqlite(value: &serde_json::Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match value {
        serde_json::Value::Null => V::Null,
        serde_json::Value::Bool(b) => V::Integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                V::Integer(i)
            } else if let Some(f) = n.as_f64() {
                V::Real(f)
            } else {
                V::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => V::Text(s.clone()),
        // JSON objects/arrays land as text — the schema-declared
        // type is the contract; we round-trip via SQLite's text
        // storage which lines up with how the framework's macro
        // path stores JSON columns.
        other => V::Text(other.to_string()),
    }
}

fn clamp_limit(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT)
}

/// Run a closure against a SQLite connection on the blocking pool. The
/// connection is locked for the duration of the closure; we don't try
/// to pool connections in Phase 1b.
async fn with_conn<F, R>(connection: Arc<Mutex<Connection>>, f: F) -> Result<R, DataError>
where
    F: FnOnce(&mut Connection) -> Result<R, DataError> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut conn = connection.blocking_lock();
        f(&mut conn)
    })
    .await
    .map_err(|e| DataError::BlockingJoin(e.to_string()))?
}

fn fetch_rows(conn: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Row>, DataError> {
    let mut stmt = conn.prepare(sql)?;
    let mut iter = stmt.query(params)?;
    let mut rows = Vec::new();
    while let Some(row) = iter.next()? {
        let text: String = row.get(0)?;
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            DataError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::other(e.to_string())),
            ))
        })?;
        if let serde_json::Value::Object(map) = value {
            rows.push(map);
        }
    }
    Ok(rows)
}

#[async_trait]
impl DataSource for SqliteSource {
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError> {
        let (resolved_model, info) = resolve_model(&self.schema, model)?;
        let limit = clamp_limit(page.limit);
        let sql = build_list_sql(&info, limit);
        let pk_field_name = find_pk_field(resolved_model)
            .map(|f| f.name.clone())
            .expect("resolve_model returns an error when there is no @id");
        let cursor_owned = page.cursor.map(str::to_owned);

        let rows = with_conn(self.connection.clone(), move |conn| match cursor_owned {
            Some(s) => fetch_rows(conn, &sql, &[&s]),
            None => fetch_rows(conn, &sql, &[&rusqlite::types::Null]),
        })
        .await?;

        let next_cursor = if rows.len() == limit as usize {
            rows.last()
                .and_then(|r| r.get(&pk_field_name))
                .map(json_value_to_cursor)
        } else {
            None
        };

        Ok(Page { rows, next_cursor })
    }

    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let sql = build_get_sql(&info);
        let pk_owned = pk.to_owned();

        let rows = with_conn(self.connection.clone(), move |conn| {
            fetch_rows(conn, &sql, &[&pk_owned])
        })
        .await?;

        Ok(rows.into_iter().next())
    }

    async fn create(&self, model: &str, payload: &Row) -> Result<Row, DataError> {
        let (resolved, info) = resolve_model(&self.schema, model)?;
        let resolved = resolved.clone();
        let (cols, sql_args) = build_payload_bindings(&info, payload, false);
        let sql = build_insert_sql(&info, &cols);
        let conn = self.connection.clone();

        let row = with_conn(conn, move |conn| {
            let params: Vec<&dyn rusqlite::ToSql> = sql_args
                .iter()
                .map(|v| v as &dyn rusqlite::ToSql)
                .collect();
            match fetch_rows(conn, &sql, &params) {
                Ok(rows) => rows
                    .into_iter()
                    .next()
                    .ok_or_else(|| DataError::Sqlite(rusqlite::Error::QueryReturnedNoRows)),
                Err(DataError::Sqlite(e)) => {
                    if let Some(mapped) = map_sqlite_error(Some(&resolved), &e) {
                        Err(mapped)
                    } else {
                        Err(DataError::Sqlite(e))
                    }
                }
                Err(other) => Err(other),
            }
        })
        .await?;
        Ok(row)
    }

    async fn update(
        &self,
        model: &str,
        pk: &str,
        payload: &Row,
    ) -> Result<Option<Row>, DataError> {
        let (resolved, info) = resolve_model(&self.schema, model)?;
        let resolved = resolved.clone();
        if payload.is_empty() {
            return self.get(model, pk).await;
        }
        let (cols, mut sql_args) = build_payload_bindings(&info, payload, true);
        sql_args.push(rusqlite::types::Value::Text(pk.to_owned()));
        let sql = build_update_sql(&info, &cols);
        let conn = self.connection.clone();

        let rows = with_conn(conn, move |conn| {
            let params: Vec<&dyn rusqlite::ToSql> = sql_args
                .iter()
                .map(|v| v as &dyn rusqlite::ToSql)
                .collect();
            match fetch_rows(conn, &sql, &params) {
                Ok(rows) => Ok(rows),
                Err(DataError::Sqlite(e)) => {
                    if let Some(mapped) = map_sqlite_error(Some(&resolved), &e) {
                        Err(mapped)
                    } else {
                        Err(DataError::Sqlite(e))
                    }
                }
                Err(other) => Err(other),
            }
        })
        .await?;
        Ok(rows.into_iter().next())
    }

    async fn delete(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        let (resolved, info) = resolve_model(&self.schema, model)?;
        let resolved = resolved.clone();
        let sql = build_delete_sql(&info);
        let pk_owned = pk.to_owned();
        let conn = self.connection.clone();
        let rows = with_conn(conn, move |conn| {
            match fetch_rows(conn, &sql, &[&pk_owned]) {
                Ok(rows) => Ok(rows),
                Err(DataError::Sqlite(e)) => {
                    if let Some(mapped) = map_sqlite_error(Some(&resolved), &e) {
                        Err(mapped)
                    } else {
                        Err(DataError::Sqlite(e))
                    }
                }
                Err(other) => Err(other),
            }
        })
        .await?;
        Ok(rows.into_iter().next())
    }

    async fn preview_sql(
        &self,
        op: SqlOp,
        model: &str,
        pk: Option<&str>,
        payload: Option<&Row>,
    ) -> Result<SqlPreview, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let (sql, params) = match op {
            SqlOp::List => (
                build_list_sql(&info, DEFAULT_PAGE_LIMIT),
                vec![SqlParam {
                    index: 1,
                    binding: "cursor (NULL on first page)".to_owned(),
                    kind: "text",
                }],
            ),
            SqlOp::Get => (
                build_get_sql(&info),
                vec![SqlParam {
                    index: 1,
                    binding: pk.unwrap_or("…").to_owned(),
                    kind: pk_kind(info.pk_cast),
                }],
            ),
            SqlOp::Create => {
                let (cols, binds) = payload
                    .map(|p| build_payload_bindings(&info, p, false))
                    .unwrap_or_else(|| sample_columns_and_binds(&info));
                let sql = build_insert_sql(&info, &cols);
                (sql, label_params(&cols, &binds, info.pk_cast, None))
            }
            SqlOp::Update => {
                let (cols, binds) = payload
                    .map(|p| build_payload_bindings(&info, p, true))
                    .unwrap_or_else(|| sample_columns_and_binds(&info));
                let mut params = label_params(&cols, &binds, info.pk_cast, None);
                params.push(SqlParam {
                    index: (cols.len() + 1) as u32,
                    binding: pk.unwrap_or("…").to_owned(),
                    kind: pk_kind(info.pk_cast),
                });
                (build_update_sql(&info, &cols), params)
            }
            SqlOp::Delete => (
                build_delete_sql(&info),
                vec![SqlParam {
                    index: 1,
                    binding: pk.unwrap_or("…").to_owned(),
                    kind: pk_kind(info.pk_cast),
                }],
            ),
        };
        Ok(SqlPreview {
            driver: "sqlite",
            sql,
            params,
            plan: None,
            notes: None,
        })
    }

    async fn inspect_columns(
        &self,
        model: &str,
    ) -> Result<Option<Vec<ColumnSnapshot>>, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let table = info.table.clone();
        let conn = self.connection.clone();
        let rows = with_conn(conn, move |conn| -> Result<Vec<ColumnSnapshot>, DataError> {
            let sql = format!("PRAGMA table_info(\"{}\")", table.replace('"', ""));
            let mut stmt = conn.prepare(&sql)?;
            let mut iter = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = iter.next()? {
                let name: String = row.get(1)?;
                let data_type: String = row.get(2).unwrap_or_default();
                let notnull: i64 = row.get(3).unwrap_or(0);
                out.push(ColumnSnapshot {
                    name,
                    data_type,
                    nullable: notnull == 0,
                });
            }
            Ok(out)
        })
        .await?;
        if rows.is_empty() { Ok(None) } else { Ok(Some(rows)) }
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
        let filter_owned = filter_value.to_owned();
        let cursor_owned = page.cursor.map(str::to_owned);

        let rows = with_conn(self.connection.clone(), move |conn| {
            match cursor_owned {
                Some(c) => fetch_rows(conn, &sql, &[&filter_owned, &c]),
                None => fetch_rows(conn, &sql, &[&filter_owned, &rusqlite::types::Null]),
            }
        })
        .await?;

        let next_cursor = if rows.len() == limit as usize {
            rows.last()
                .and_then(|r| r.get(&pk_field_name))
                .map(json_value_to_cursor)
        } else {
            None
        };

        Ok(Page { rows, next_cursor })
    }
}

#[allow(dead_code)]
fn _silence_unused_column_info(_c: &ColumnInfo<'_>) {}

pub(crate) fn pk_kind(cast: PkCast) -> &'static str {
    match cast {
        PkCast::Text => "text",
        PkCast::BigInt => "integer",
    }
}

fn sample_columns_and_binds(
    info: &ModelSqlInfo<'_>,
) -> (Vec<String>, Vec<rusqlite::types::Value>) {
    let cols = info.columns.iter().map(|c| c.column_name.clone()).collect();
    let binds = info
        .columns
        .iter()
        .map(|_| rusqlite::types::Value::Text("…".to_owned()))
        .collect();
    (cols, binds)
}

fn label_params(
    cols: &[String],
    binds: &[rusqlite::types::Value],
    _pk: PkCast,
    _: Option<()>,
) -> Vec<SqlParam> {
    cols.iter()
        .zip(binds.iter())
        .enumerate()
        .map(|(i, (col, value))| SqlParam {
            index: (i + 1) as u32,
            binding: col.clone(),
            kind: sqlite_kind(value),
        })
        .collect()
}

fn sqlite_kind(value: &rusqlite::types::Value) -> &'static str {
    use rusqlite::types::Value as V;
    match value {
        V::Null => "null",
        V::Integer(_) => "integer",
        V::Real(_) => "real",
        V::Text(_) => "text",
        V::Blob(_) => "blob",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Schema {
        cratestack_parser::parse_schema(text).expect("schema parses")
    }

    fn make_source(schema: Schema) -> SqliteSource {
        let conn = Connection::open_in_memory().expect("open sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
            INSERT INTO customers (id, email) VALUES
              (1, 'a@example.com'),
              (2, 'b@example.com'),
              (3, 'c@example.com');

            CREATE TABLE posts (
              id TEXT PRIMARY KEY,
              author_id INTEGER NOT NULL,
              title TEXT NOT NULL
            );
            INSERT INTO posts (id, author_id, title) VALUES
              ('p1', 1, 'first'),
              ('p2', 1, 'second'),
              ('p3', 2, 'third');
            "#,
        )
        .expect("ddl");
        SqliteSource::new(conn, Arc::new(schema))
    }

    const BLOG_SCHEMA: &str = r#"
        model Customer {
          id Int @id
          email String
          posts Post[] @relation(fields: [id], references: [authorId])
        }

        model Post {
          id String @id
          authorId Int
          title String
          author Customer @relation(fields: [authorId], references: [id])
        }
    "#;

    #[test]
    fn list_sql_uses_text_cursor_for_string_pk() {
        let schema = parse(BLOG_SCHEMA);
        let (_, info) = resolve_model(&schema, "Post").unwrap();
        let sql = build_list_sql(&info, 25);
        assert!(sql.contains(r#""id" > ?1"#), "{sql}");
        assert!(!sql.contains("CAST"), "{sql}");
        assert!(sql.contains("LIMIT 25"), "{sql}");
        assert!(sql.contains(r#"FROM "posts""#), "{sql}");
    }

    #[test]
    fn list_sql_casts_int_pk_via_cast_as_integer() {
        let schema = parse(BLOG_SCHEMA);
        let (_, info) = resolve_model(&schema, "Customer").unwrap();
        let sql = build_list_sql(&info, 10);
        assert!(
            sql.contains(r#""id" > CAST(?1 AS INTEGER)"#),
            "{sql}"
        );
    }

    #[test]
    fn json_object_emits_field_name_aliases() {
        let schema = parse(BLOG_SCHEMA);
        let (_, info) = resolve_model(&schema, "Post").unwrap();
        let object = build_json_object(&info);
        assert!(object.contains("'id', \"id\""), "{object}");
        assert!(object.contains("'authorId', \"author_id\""), "{object}");
        assert!(object.contains("'title', \"title\""), "{object}");
    }

    #[tokio::test]
    async fn list_returns_rows_with_field_name_aliases() {
        let schema = parse(BLOG_SCHEMA);
        let source = make_source(schema);
        let page = source
            .list("Post", PageRequest::default())
            .await
            .expect("list ok");
        assert_eq!(page.rows.len(), 3);
        let first = &page.rows[0];
        assert_eq!(first.get("id").unwrap(), &serde_json::json!("p1"));
        assert_eq!(first.get("authorId").unwrap(), &serde_json::json!(1));
        assert_eq!(first.get("title").unwrap(), &serde_json::json!("first"));
    }

    #[tokio::test]
    async fn list_paginates_with_cursor() {
        let schema = parse(BLOG_SCHEMA);
        let source = make_source(schema);
        let page1 = source
            .list(
                "Post",
                PageRequest {
                    cursor: None,
                    limit: Some(2),
                },
            )
            .await
            .expect("first page");
        assert_eq!(page1.rows.len(), 2);
        assert_eq!(page1.next_cursor.as_deref(), Some("p2"));

        let page2 = source
            .list(
                "Post",
                PageRequest {
                    cursor: Some("p2"),
                    limit: Some(2),
                },
            )
            .await
            .expect("second page");
        assert_eq!(page2.rows.len(), 1);
        assert!(page2.next_cursor.is_none());
    }

    #[tokio::test]
    async fn get_returns_single_row_by_pk() {
        let schema = parse(BLOG_SCHEMA);
        let source = make_source(schema);
        let row = source.get("Post", "p2").await.expect("get").expect("some");
        assert_eq!(row.get("title").unwrap(), &serde_json::json!("second"));
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_pk() {
        let schema = parse(BLOG_SCHEMA);
        let source = make_source(schema);
        let row = source.get("Post", "missing").await.expect("get ok");
        assert!(row.is_none());
    }

    #[tokio::test]
    async fn follow_returns_rows_matching_filter_column() {
        let schema = parse(BLOG_SCHEMA);
        let source = make_source(schema);
        let page = source
            .follow("Post", "author_id", PkCast::BigInt, "1", PageRequest::default())
            .await
            .expect("follow ok");
        assert_eq!(page.rows.len(), 2);
        let titles: Vec<&str> = page
            .rows
            .iter()
            .filter_map(|r| r.get("title").and_then(|v| v.as_str()))
            .collect();
        assert!(titles.contains(&"first"));
        assert!(titles.contains(&"second"));
    }
}
