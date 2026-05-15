//! Per-model ORM delegate — the sync mirror of `cratestack-sqlx::ModelDelegate`.

use std::marker::PhantomData;

use cratestack_core::BatchSummary;
use cratestack_sql::{
    ConflictTarget, CreateModelInput, Filter, FilterExpr, IntoSqlValue, ModelDescriptor,
    OrderClause, SqliteDialect, SqlValue, UpdateModelInput, UpsertModelInput,
};
use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_delete,
    render::render_insert, render::render_select, render::render_select_by_pk,
    render::render_update, render::render_update_many, render::render_upsert_with_conflict,
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
            conflict_target: ConflictTarget::PrimaryKey,
        }
    }

    pub fn update(&self, id: PK) -> UpdateRecord<'a, M, PK> {
        UpdateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    /// Bulk UPDATE by predicate. Mirrors the sqlx delegate; the embedded
    /// layer has no policies, so the only filter applied beyond the
    /// caller's is the implicit soft-delete-IS-NULL where applicable.
    /// Refuses to run without at least one filter — same safety stance.
    pub fn update_many(&self) -> UpdateMany<'a, M, PK> {
        UpdateMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
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

    /// Conditionally append a filter — `None` is a no-op. Mirrors
    /// the sqlx delegate's `where_optional` so cross-backend code can
    /// stay backend-agnostic when handling optional query
    /// parameters.
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
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

    /// API-compat no-op. SQLite has no `SELECT ... FOR UPDATE` — its
    /// transaction model uses whole-database write locks (`BEGIN IMMEDIATE`),
    /// which already give the serialization guarantees the server-side
    /// `FOR UPDATE` is reaching for. Kept on the embedded delegate so
    /// schemas can compile and tests can share code across backends.
    pub fn for_update(self) -> Self {
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

    /// Run against a caller-supplied connection (typically the active
    /// transaction's connection, via `&mut *tx`). Mirrors the sqlx
    /// `run_in_tx` shape for cross-backend ergonomics; on rusqlite this
    /// is just the same query executed against the provided connection
    /// instead of the runtime's mutex-guarded one.
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<Vec<M>, RusqliteError>
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
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let rows = stmt
            .query_map(params_from_iter(bind_iter), |row| {
                M::from_rusqlite_row(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
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
    /// See [`FindMany::for_update`] — no-op on the embedded layer.
    pub fn for_update(self) -> Self {
        self
    }

    /// API-compat no-op. The embedded layer doesn't enforce policies,
    /// so the detail-vs-list distinction has no runtime effect; kept
    /// so cross-backend code can call `.as_detail()` / `.as_list()`
    /// without conditional compilation.
    pub fn as_detail(self) -> Self {
        self
    }

    /// API-compat no-op. See [`Self::as_detail`].
    pub fn as_list(self) -> Self {
        self
    }

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

    /// Run against a caller-supplied connection. See
    /// [`FindMany::run_in_tx`] for cross-backend rationale.
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<Option<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) =
            render_select_by_pk(&dialect, self.descriptor, self.id.clone().into_sql_value());
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let mut rows = stmt.query(params_from_iter(bind_iter))?;
        if let Some(row) = rows.next()? {
            Ok(Some(M::from_rusqlite_row(row)?))
        } else {
            Ok(None)
        }
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
            run_insert_returning(conn, &sql, &binds)
        })
    }

    /// Run against a caller-supplied connection (typically the active
    /// transaction's connection, via `&mut *tx`). Mirrors the sqlx
    /// `run_in_tx` shape so cross-backend code can switch backends
    /// without rewriting transaction call sites.
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_insert(&dialect, self.descriptor, &values);
        run_insert_returning(conn, &sql, &binds)
    }
}

fn run_insert_returning<M: FromRusqliteRow>(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: &[SqlValue],
) -> Result<M, RusqliteError> {
    let mut stmt = conn.prepare(sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    let row = rows.next()?.ok_or(RusqliteError::NotFound)?;
    Ok(M::from_rusqlite_row(row)?)
}

pub struct UpsertRecord<'a, M: 'static, PK: 'static, I> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    conflict_target: ConflictTarget,
}

impl<'a, M: 'static, PK: 'static, I> UpsertRecord<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// Choose the conflict target. See
    /// [`cratestack_sqlx::UpsertRecord::on_conflict`]; the embedded
    /// runtime supports `ConflictTarget::Columns` symmetrically.
    pub fn on_conflict(mut self, target: ConflictTarget) -> Self {
        self.conflict_target = target;
        self
    }

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_upsert_with_conflict(
            &dialect,
            self.descriptor,
            &values,
            self.conflict_target,
        );
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
        let (sql, binds) = render_upsert_with_conflict(
            &dialect,
            self.descriptor,
            &values,
            self.conflict_target,
        );
        self.runtime
            .with_connection(|conn| run_insert_returning(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_upsert_with_conflict(
            &dialect,
            self.descriptor,
            &values,
            self.conflict_target,
        );
        run_insert_returning(conn, &sql, &binds)
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
        self.runtime
            .with_connection(|conn| run_insert_returning(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_update(&dialect, self.descriptor, &values, self.id.clone().into_sql_value());
        run_insert_returning(conn, &sql, &binds)
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
        self.runtime
            .with_connection(|conn| run_insert_returning(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<M, RusqliteError>
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
        run_insert_returning(conn, &sql, &binds)
    }
}

// ───── UpdateMany ──────────────────────────────────────────────────────────

pub struct UpdateMany<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> UpdateMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    /// Conditionally append a filter. See
    /// [`FindMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    pub fn set<I>(self, input: I) -> UpdateManySet<'a, M, PK, I> {
        UpdateManySet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: self.filters,
            input,
            _marker: PhantomData,
        }
    }
}

pub struct UpdateManySet<'a, M: 'static, PK: 'static, I> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
    input: I,
    _marker: PhantomData<fn() -> M>,
}

impl<'a, M: 'static, PK: 'static, I> UpdateManySet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_update_many(&dialect, self.descriptor, &values, &self.filters);
        sql
    }

    /// Run the bulk update. Returns a `BatchSummary { total, ok, err: 0 }`
    /// where `ok` is the number of rows the UPDATE actually mutated.
    pub fn run(self) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        if self.filters.is_empty() {
            // Mirror the sqlx safety stance: reject predicate-less bulk
            // updates loud and early. There's no equivalent of
            // `CoolError::Validation` here, so we surface a sqlite error
            // — an empty WHERE would let a typo wipe the table.
            return Err(RusqliteError::Validation(
                "update_many requires at least one filter".to_owned(),
            ));
        }
        let values = self.input.sql_values();
        if values.is_empty() {
            return Err(RusqliteError::Validation(
                "update input must contain at least one changed column".to_owned(),
            ));
        }
        let (sql, binds) = render_update_many(&dialect, self.descriptor, &values, &self.filters);
        self.runtime
            .with_connection(|conn| run_update_many_returning::<M>(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        if self.filters.is_empty() {
            return Err(RusqliteError::Validation(
                "update_many requires at least one filter".to_owned(),
            ));
        }
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        if values.is_empty() {
            return Err(RusqliteError::Validation(
                "update input must contain at least one changed column".to_owned(),
            ));
        }
        let (sql, binds) = render_update_many(&dialect, self.descriptor, &values, &self.filters);
        run_update_many_returning::<M>(conn, &sql, &binds)
    }
}

fn run_update_many_returning<M: FromRusqliteRow>(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: &[SqlValue],
) -> Result<BatchSummary, RusqliteError> {
    let mut stmt = conn.prepare(sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut count = 0usize;
    {
        // We use query_map but only care about the row count — discarding
        // each row keeps the FromRusqliteRow round-trip honest (catches
        // schema mismatches early) without retaining the materialised set.
        let iter = stmt.query_map(params_from_iter(bind_iter), |row| {
            M::from_rusqlite_row(row).map(|_| ())
        })?;
        for item in iter {
            item?;
            count += 1;
        }
    }
    Ok(BatchSummary {
        total: count,
        ok: count,
        err: 0,
    })
}
