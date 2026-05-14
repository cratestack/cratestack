//! Batch primitives — embedded mirror of `cratestack-sqlx`'s batch module.
//!
//! The embedded layer trims the surface: no policies, no audit, no event
//! outbox. That makes the implementation noticeably simpler than the
//! server side — two single-statement ops (`batch_get` / `batch_delete`)
//! and three savepointed loops (`batch_create` / `batch_update` /
//! `batch_upsert`).
//!
//! Error vocabulary: per-item failures surface as
//! `BatchItemError { code: "DATABASE_ERROR", ... }` (or `"CONFLICT"` for
//! unique-constraint violations on create / upsert), matching the codes
//! the server side projects from `CoolError`. This keeps cross-platform
//! clients on a single error-code table whether the response came from
//! sqlx or rusqlite.

use std::collections::HashMap;
use std::hash::Hash;

use cratestack_core::{
    BATCH_MAX_ITEMS, BatchItemError, BatchItemResult, BatchItemStatus, BatchResponse,
    BatchSummary, find_duplicate_position,
};
use cratestack_sql::{
    CreateModelInput, IntoSqlValue, ModelDescriptor, ModelPrimaryKey, SqlValue,
    UpdateModelInput, UpsertModelInput, find_duplicate_sql_value,
};

use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_upsert,
};

// ───── outer guards ─────────────────────────────────────────────────────────

fn validate_batch_size(len: usize) -> Result<(), RusqliteError> {
    if len > BATCH_MAX_ITEMS {
        return Err(RusqliteError::BatchTooLarge {
            actual: len,
            maximum: BATCH_MAX_ITEMS,
        });
    }
    Ok(())
}

fn reject_duplicate_pks<K: Eq + Hash + Clone>(keys: &[K]) -> Result<(), RusqliteError> {
    if let Some((first, dup)) = find_duplicate_position(keys.iter().cloned()) {
        return Err(RusqliteError::DuplicateBatchKey { first, duplicate: dup });
    }
    Ok(())
}

fn reject_duplicate_sql_values(values: &[SqlValue]) -> Result<(), RusqliteError> {
    if let Some((first, dup)) = find_duplicate_sql_value(values) {
        return Err(RusqliteError::DuplicateBatchKey { first, duplicate: dup });
    }
    Ok(())
}

/// Build a per-item error envelope from a rusqlite error. Recognises any
/// constraint violation as `CONFLICT` so the wire shape matches the server
/// side's projection for unique-key, PK, and CHECK failures alike. SQLite
/// uses different extended codes for SQLITE_CONSTRAINT_UNIQUE (2067) and
/// SQLITE_CONSTRAINT_PRIMARYKEY (1555); matching on the broad code lets
/// either land as `CONFLICT` without us having to enumerate every subtype.
fn item_error(error: rusqlite::Error) -> BatchItemError {
    let is_constraint_violation = matches!(
        &error,
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation
    );
    BatchItemError {
        code: if is_constraint_violation {
            "CONFLICT".to_owned()
        } else {
            "DATABASE_ERROR".to_owned()
        },
        message: error.to_string(),
    }
}

fn ok_item<T>(index: usize, value: T) -> BatchItemResult<T> {
    BatchItemResult {
        index,
        status: BatchItemStatus::Ok { value },
    }
}

fn err_item<T>(index: usize, error: BatchItemError) -> BatchItemResult<T> {
    BatchItemResult {
        index,
        status: BatchItemStatus::Error { error },
    }
}

fn finalize<T>(results: Vec<BatchItemResult<T>>) -> BatchResponse<T> {
    let total = results.len();
    let ok = results
        .iter()
        .filter(|r| matches!(r.status, BatchItemStatus::Ok { .. }))
        .count();
    BatchResponse {
        results,
        summary: BatchSummary {
            total,
            ok,
            err: total - ok,
        },
    }
}

// ───── BatchGet ─────────────────────────────────────────────────────────────

pub struct BatchGet<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) ids: Vec<PK>,
}

impl<'a, M: 'static, PK: 'static> BatchGet<'a, M, PK> {
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow + ModelPrimaryKey<PK>,
        PK: IntoSqlValue + Clone + Eq + Hash,
    {
        validate_batch_size(self.ids.len())?;
        reject_duplicate_pks(&self.ids)?;
        if self.ids.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        // `render_select` already understands soft-delete; build an IN
        // filter manually and bind the PK list positionally.
        let mut sql = format!(
            "SELECT {} FROM {} WHERE ",
            self.descriptor.select_projection(),
            self.descriptor.table_name,
        );
        if let Some(col) = self.descriptor.soft_delete_column {
            sql.push_str(col);
            sql.push_str(" IS NULL AND ");
        }
        sql.push_str(self.descriptor.primary_key);
        sql.push_str(" IN (");
        for index in 0..self.ids.len() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!("?{}", index + 1));
        }
        sql.push(')');

        let binds: Vec<SqlValue> = self.ids.iter().cloned().map(IntoSqlValue::into_sql_value).collect();

        let rows: Vec<M> = self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let rows = stmt
                .query_map(params_from_iter(bind_iter), |row| {
                    M::from_rusqlite_row(row)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;

        let mut by_pk: HashMap<PK, M> =
            rows.into_iter().map(|m| (m.primary_key(), m)).collect();
        let results = self
            .ids
            .into_iter()
            .enumerate()
            .map(|(index, id)| match by_pk.remove(&id) {
                Some(record) => ok_item(index, record),
                None => err_item(
                    index,
                    BatchItemError {
                        code: "NOT_FOUND".to_owned(),
                        message: "no row matched".to_owned(),
                    },
                ),
            })
            .collect();
        Ok(finalize(results))
    }
}

// ───── BatchDelete ──────────────────────────────────────────────────────────

pub struct BatchDelete<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) ids: Vec<PK>,
}

impl<'a, M: 'static, PK: 'static> BatchDelete<'a, M, PK> {
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow + ModelPrimaryKey<PK>,
        PK: IntoSqlValue + Clone + Eq + Hash,
    {
        validate_batch_size(self.ids.len())?;
        reject_duplicate_pks(&self.ids)?;
        if self.ids.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        // Soft-delete becomes an UPDATE-of-deleted_at; hard delete uses
        // DELETE. Either way, RETURNING gives us the deleted rows so we
        // can pair them back to the input order.
        let mut sql = match self.descriptor.soft_delete_column {
            Some(col) => {
                let mut s =
                    format!("UPDATE {} SET {col} = CURRENT_TIMESTAMP", self.descriptor.table_name);
                if let Some(version_col) = self.descriptor.version_column {
                    s.push_str(&format!(", {version_col} = {version_col} + 1"));
                }
                s.push_str(&format!(" WHERE {col} IS NULL AND "));
                s
            }
            None => format!("DELETE FROM {} WHERE ", self.descriptor.table_name),
        };
        sql.push_str(self.descriptor.primary_key);
        sql.push_str(" IN (");
        for index in 0..self.ids.len() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!("?{}", index + 1));
        }
        sql.push_str(") RETURNING ");
        sql.push_str(&self.descriptor.select_projection());

        let binds: Vec<SqlValue> = self.ids.iter().cloned().map(IntoSqlValue::into_sql_value).collect();

        let deleted: Vec<M> = self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let rows = stmt
                .query_map(params_from_iter(bind_iter), |row| {
                    M::from_rusqlite_row(row)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;

        let mut by_pk: HashMap<PK, M> =
            deleted.into_iter().map(|m| (m.primary_key(), m)).collect();
        let results = self
            .ids
            .into_iter()
            .enumerate()
            .map(|(index, id)| match by_pk.remove(&id) {
                Some(record) => ok_item(index, record),
                None => err_item(
                    index,
                    BatchItemError {
                        code: "NOT_FOUND".to_owned(),
                        message: "no row matched".to_owned(),
                    },
                ),
            })
            .collect();
        Ok(finalize(results))
    }
}

// ───── BatchCreate ──────────────────────────────────────────────────────────

pub struct BatchCreate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchCreate<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        validate_batch_size(self.inputs.len())?;
        if self.inputs.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        self.runtime.with_connection(|conn| {
            let mut tx = conn.transaction()?;
            let mut results = Vec::with_capacity(self.inputs.len());
            for (index, input) in self.inputs.into_iter().enumerate() {
                let mut sp = tx.savepoint()?;
                match insert_one(&mut sp, self.descriptor, &input) {
                    Ok(record) => {
                        sp.commit()?;
                        results.push(ok_item(index, record));
                    }
                    Err(error) => {
                        sp.rollback()?;
                        results.push(err_item(index, item_error(error)));
                    }
                }
            }
            tx.commit()?;
            Ok(finalize(results))
        })
    }
}

fn insert_one<M, PK, I>(
    sp: &mut rusqlite::Savepoint<'_>,
    descriptor: &ModelDescriptor<M, PK>,
    input: &I,
) -> rusqlite::Result<M>
where
    I: CreateModelInput<M>,
    M: FromRusqliteRow,
{
    let values = input.sql_values();
    let mut sql = format!("INSERT INTO {} (", descriptor.table_name);
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(value.column);
    }
    sql.push_str(") VALUES (");
    for idx in 0..values.len() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&format!("?{}", idx + 1));
    }
    sql.push_str(") RETURNING ");
    sql.push_str(&descriptor.select_projection());

    let binds: Vec<SqlValue> = values.iter().map(|v| v.value.clone()).collect();
    let mut stmt = sp.prepare(&sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    let row = rows
        .next()?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
    M::from_rusqlite_row(row)
}

// ───── BatchUpdate ──────────────────────────────────────────────────────────

/// `(id, patch)` per item. The embedded layer doesn't enforce policy or
/// `@version`, so there's no `if_match` slot — that's a server-only
/// concern in v1.
pub type BatchUpdateItem<PK, I> = (PK, I);

pub struct BatchUpdate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) items: Vec<BatchUpdateItem<PK, I>>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpdate<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
    PK: Clone + Eq + Hash + IntoSqlValue,
{
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        validate_batch_size(self.items.len())?;
        let ids: Vec<PK> = self.items.iter().map(|(id, _)| id.clone()).collect();
        reject_duplicate_pks(&ids)?;
        if self.items.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        self.runtime.with_connection(|conn| {
            let mut tx = conn.transaction()?;
            let mut results = Vec::with_capacity(self.items.len());
            for (index, (id, input)) in self.items.into_iter().enumerate() {
                let mut sp = tx.savepoint()?;
                match update_one(&mut sp, self.descriptor, id, &input) {
                    Ok(Some(record)) => {
                        sp.commit()?;
                        results.push(ok_item(index, record));
                    }
                    Ok(None) => {
                        sp.rollback()?;
                        results.push(err_item(
                            index,
                            BatchItemError {
                                code: "NOT_FOUND".to_owned(),
                                message: "no row matched".to_owned(),
                            },
                        ));
                    }
                    Err(error) => {
                        sp.rollback()?;
                        results.push(err_item(index, item_error(error)));
                    }
                }
            }
            tx.commit()?;
            Ok(finalize(results))
        })
    }
}

fn update_one<M, PK, I>(
    sp: &mut rusqlite::Savepoint<'_>,
    descriptor: &ModelDescriptor<M, PK>,
    id: PK,
    input: &I,
) -> rusqlite::Result<Option<M>>
where
    I: UpdateModelInput<M>,
    M: FromRusqliteRow,
    PK: IntoSqlValue,
{
    let values = input.sql_values();
    if values.is_empty() {
        // Empty patch isn't a database failure — surface it as NotFound-
        // adjacent at the call site via Ok(None) so the envelope reports
        // a per-item NOT_FOUND. (We could instead introduce a per-item
        // VALIDATION code here, but staying conservative: NOT_FOUND keeps
        // the wire surface small.)
        return Ok(None);
    }

    let mut sql = format!("UPDATE {} SET ", descriptor.table_name);
    let mut binds: Vec<SqlValue> = Vec::with_capacity(values.len() + 1);
    let mut bind_index = 1usize;
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&format!("{} = ?{}", value.column, bind_index));
        bind_index += 1;
        binds.push(value.value.clone());
    }
    if let Some(version_col) = descriptor.version_column {
        sql.push_str(&format!(", {version_col} = {version_col} + 1"));
    }
    sql.push_str(&format!(" WHERE {} = ?{}", descriptor.primary_key, bind_index));
    binds.push(id.into_sql_value());
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());

    let mut stmt = sp.prepare(&sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    match rows.next()? {
        Some(row) => Ok(Some(M::from_rusqlite_row(row)?)),
        None => Ok(None),
    }
}

// ───── BatchUpsert ──────────────────────────────────────────────────────────

pub struct BatchUpsert<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpsert<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        validate_batch_size(self.inputs.len())?;
        let pks: Vec<SqlValue> = self
            .inputs
            .iter()
            .map(UpsertModelInput::primary_key_value)
            .collect();
        reject_duplicate_sql_values(&pks)?;
        if self.inputs.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        self.runtime.with_connection(|conn| {
            let mut tx = conn.transaction()?;
            let mut results = Vec::with_capacity(self.inputs.len());
            for (index, input) in self.inputs.into_iter().enumerate() {
                let mut sp = tx.savepoint()?;
                match upsert_one(&mut sp, self.descriptor, &input) {
                    Ok(record) => {
                        sp.commit()?;
                        results.push(ok_item(index, record));
                    }
                    Err(error) => {
                        sp.rollback()?;
                        results.push(err_item(index, item_error(error)));
                    }
                }
            }
            tx.commit()?;
            Ok(finalize(results))
        })
    }
}

fn upsert_one<M, PK, I>(
    sp: &mut rusqlite::Savepoint<'_>,
    descriptor: &ModelDescriptor<M, PK>,
    input: &I,
) -> rusqlite::Result<M>
where
    I: UpsertModelInput<M>,
    M: FromRusqliteRow,
{
    let dialect = cratestack_sql::SqliteDialect;
    let values = input.sql_values();
    let (sql, binds) = render_upsert(&dialect, descriptor, &values);
    let mut stmt = sp.prepare(&sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    let row = rows
        .next()?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
    M::from_rusqlite_row(row)
}

