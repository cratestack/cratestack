//! Per-model ORM delegate — the sync mirror of `cratestack-sqlx::ModelDelegate`.

use std::marker::PhantomData;

use cratestack_sql::{
    CreateModelInput, Filter, FilterExpr, IntoSqlValue, ModelDescriptor, OrderClause,
    SqliteDialect, SqlValue, UpdateModelInput, UpsertModelInput,
};
use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_delete,
    render::render_insert, render::render_select, render::render_select_by_pk,
    render::render_update, render::render_upsert,
};

#[derive(Clone, Copy)]
pub struct ModelDelegate<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
}

impl<'a, M: 'static, PK: 'static> ModelDelegate<'a, M, PK> {
    pub fn new(
        runtime: &'a RusqliteRuntime,
        descriptor: &'static ModelDescriptor<M, PK>,
    ) -> Self {
        Self {
            runtime,
            descriptor,
        }
    }

    pub fn descriptor(&self) -> &'static ModelDescriptor<M, PK> {
        self.descriptor
    }

    pub fn find_many(&self) -> FindMany<'a, M, PK> {
        FindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    pub fn find_unique(&self, id: PK) -> FindUnique<'a, M, PK>
    where
        PK: IntoSqlValue + Clone,
    {
        FindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    pub fn create<I>(&self, input: I) -> CreateRecord<'a, M, PK, I> {
        CreateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input,
        }
    }

    /// Insert-or-update on primary-key conflict. Only models with a client-
    /// supplied `@id` (no `@default(...)`) implement `UpsertModelInput`, so
    /// `.upsert(...)` on a server-PK model is a compile error — same as the
    /// sqlx delegate.
    pub fn upsert<I>(&self, input: I) -> UpsertRecord<'a, M, PK, I> {
        UpsertRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input,
        }
    }

    pub fn update(&self, id: PK) -> UpdateRecord<'a, M, PK> {
        UpdateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    pub fn delete(&self, id: PK) -> DeleteRecord<'a, M, PK> {
        DeleteRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    /// Fetch many rows by primary key in one round-trip. Missing rows
    /// surface as per-item `NOT_FOUND` in the envelope; the call as a
    /// whole only fails on outer infra errors (size cap, dup keys, DB
    /// lock).
    pub fn batch_get(&self, ids: Vec<PK>) -> crate::BatchGet<'a, M, PK> {
        crate::BatchGet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            ids,
        }
    }

    /// Insert many rows in one transaction with per-item SAVEPOINTs.
    pub fn batch_create<I>(&self, inputs: Vec<I>) -> crate::BatchCreate<'a, M, PK, I> {
        crate::BatchCreate {
            runtime: self.runtime,
            descriptor: self.descriptor,
            inputs,
        }
    }

    /// Update many rows with per-item patches. No `if_match` on the embedded
    /// layer in v1 — the on-device runtime doesn't enforce `@version`.
    pub fn batch_update<I>(
        &self,
        items: Vec<crate::BatchUpdateItem<PK, I>>,
    ) -> crate::BatchUpdate<'a, M, PK, I> {
        crate::BatchUpdate {
            runtime: self.runtime,
            descriptor: self.descriptor,
            items,
        }
    }

    /// Delete many rows by primary key in one statement. Missing rows
    /// (and already-tombstoned rows on soft-delete models) surface as
    /// per-item `NOT_FOUND`.
    pub fn batch_delete(&self, ids: Vec<PK>) -> crate::BatchDelete<'a, M, PK> {
        crate::BatchDelete {
            runtime: self.runtime,
            descriptor: self.descriptor,
            ids,
        }
    }

    /// Insert-or-update many rows in one transaction with per-item
    /// SAVEPOINTs. Eligible only for models whose `@id` is client-supplied
    /// — same compile-time gate as the single-row `.upsert(...)`.
    pub fn batch_upsert<I>(&self, inputs: Vec<I>) -> crate::BatchUpsert<'a, M, PK, I> {
        crate::BatchUpsert {
            runtime: self.runtime,
            descriptor: self.descriptor,
            inputs,
        }
    }
}

pub struct FindMany<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
    order_by: Vec<OrderClause>,
    limit: Option<i64>,
    offset: Option<i64>,
}

impl<'a, M: 'static, PK: 'static> FindMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.order_by.push(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) = render_select(
            &dialect,
            self.descriptor,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        sql
    }

    pub fn run(self) -> Result<Vec<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) = render_select(
            &dialect,
            self.descriptor,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let rows = stmt
                .query_map(params_from_iter(bind_iter), |row| {
                    M::from_rusqlite_row(row)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }
}

pub struct FindUnique<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
}

impl<'a, M: 'static, PK: 'static> FindUnique<'a, M, PK>
where
    PK: IntoSqlValue + Clone,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) =
            render_select_by_pk(&dialect, self.descriptor, self.id.clone().into_sql_value());
        sql
    }

    pub fn run(self) -> Result<Option<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) =
            render_select_by_pk(&dialect, self.descriptor, self.id.clone().into_sql_value());
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            if let Some(row) = rows.next()? {
                Ok(Some(M::from_rusqlite_row(row)?))
            } else {
                Ok(None)
            }
        })
    }
}

pub struct CreateRecord<'a, M: 'static, PK: 'static, I> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
}

impl<'a, M: 'static, PK: 'static, I> CreateRecord<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_insert(&dialect, self.descriptor, &values);
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_insert(&dialect, self.descriptor, &values);
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            let row = rows.next()?.ok_or(RusqliteError::NotFound)?;
            Ok(M::from_rusqlite_row(row)?)
        })
    }
}

pub struct UpsertRecord<'a, M: 'static, PK: 'static, I> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
}

impl<'a, M: 'static, PK: 'static, I> UpsertRecord<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_upsert(&dialect, self.descriptor, &values);
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        // Validation is server-side concern only; the rusqlite layer matches
        // `CreateRecord::run`, which also skips `validate()`.
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_upsert(&dialect, self.descriptor, &values);
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            let row = rows.next()?.ok_or(RusqliteError::NotFound)?;
            Ok(M::from_rusqlite_row(row)?)
        })
    }
}

pub struct UpdateRecord<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
}

impl<'a, M: 'static, PK: 'static> UpdateRecord<'a, M, PK> {
    pub fn set<I>(self, input: I) -> UpdateRecordSet<'a, M, PK, I> {
        UpdateRecordSet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            input,
            _marker: PhantomData,
        }
    }
}

pub struct UpdateRecordSet<'a, M: 'static, PK: 'static, I> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    _marker: PhantomData<fn() -> M>,
}

impl<'a, M: 'static, PK: 'static, I> UpdateRecordSet<'a, M, PK, I>
where
    PK: IntoSqlValue + Clone,
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_update(&dialect, self.descriptor, &values, self.id.clone().into_sql_value());
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_update(&dialect, self.descriptor, &values, self.id.clone().into_sql_value());
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            let row = rows.next()?.ok_or(RusqliteError::NotFound)?;
            Ok(M::from_rusqlite_row(row)?)
        })
    }
}

pub struct DeleteRecord<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
}

impl<'a, M: 'static, PK: 'static> DeleteRecord<'a, M, PK>
where
    PK: IntoSqlValue + Clone,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) = render_delete(
            &dialect,
            self.descriptor,
            self.id.clone().into_sql_value(),
            chrono::Utc::now(),
        );
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) = render_delete(
            &dialect,
            self.descriptor,
            self.id.clone().into_sql_value(),
            chrono::Utc::now(),
        );
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            let row = rows.next()?.ok_or(RusqliteError::NotFound)?;
            let result: SqlValue = SqlValue::Int(0);
            let _ = result;
            Ok(M::from_rusqlite_row(row)?)
        })
    }
}
