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
    render::render_delete_many, render::render_insert, render::render_select,
    render::render_select_by_pk, render::render_update, render::render_update_many,
    render::render_upsert_with_conflict,
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

    /// Bulk DELETE by predicate. Soft-delete-aware (tombstones via
    /// `deleted_at = CURRENT_TIMESTAMP` when the model declares one,
    /// otherwise hard-deletes). Refuses to run without ≥1 filter —
    /// same safety stance as `update_many`.
    pub fn delete_many(&self) -> DeleteMany<'a, M, PK> {
        DeleteMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
        }
    }

    /// Aggregate read. Mirrors the sqlx delegate; the embedded layer
    /// has no policy enforcement, so the result describes every row
    /// that matches the filters and is not soft-deleted.
    pub fn aggregate(&self) -> Aggregate<'a, M, PK> {
        Aggregate {
            runtime: self.runtime,
            descriptor: self.descriptor,
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

    /// Side-load a to-one relation. See [`cratestack_sqlx::FindMany::include`]
    /// for the rationale; the embedded mirror uses the same two-step
    /// approach (parent query + IN-list child query, merge in memory).
    pub fn include<Rel: 'static, RelPK: 'static>(
        self,
        relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ) -> FindManyWith<'a, M, PK, Rel, RelPK> {
        FindManyWith {
            parent: self,
            relation,
        }
    }
}

pub struct FindManyWith<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static> {
    parent: FindMany<'a, M, PK>,
    relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
}

impl<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static>
    FindManyWith<'a, M, PK, Rel, RelPK>
{
    pub fn where_(mut self, filter: Filter) -> Self {
        self.parent = self.parent.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.parent = self.parent.where_expr(filter);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.parent = self.parent.where_optional(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.parent = self.parent.order_by(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.parent = self.parent.limit(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.parent = self.parent.offset(offset);
        self
    }

    pub fn run(self) -> Result<Vec<(M, Option<Rel>)>, RusqliteError>
    where
        M: FromRusqliteRow + Clone,
        Rel: FromRusqliteRow + Clone + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Clone + std::cmp::Eq + std::hash::Hash + IntoSqlValue,
    {
        let runtime = self.parent.runtime;
        let relation = self.relation;
        let parents = self.parent.run()?;
        run_side_load(runtime, parents, relation, None::<&rusqlite::Connection>)
    }

    pub fn run_in_tx(
        self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<(M, Option<Rel>)>, RusqliteError>
    where
        M: FromRusqliteRow + Clone,
        Rel: FromRusqliteRow + Clone + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Clone + std::cmp::Eq + std::hash::Hash + IntoSqlValue,
    {
        let runtime = self.parent.runtime;
        let relation = self.relation;
        let parents = self.parent.run_in_tx(conn)?;
        run_side_load(runtime, parents, relation, Some(conn))
    }
}

fn run_side_load<M, Rel, RelPK>(
    runtime: &RusqliteRuntime,
    parents: Vec<M>,
    relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    conn: Option<&rusqlite::Connection>,
) -> Result<Vec<(M, Option<Rel>)>, RusqliteError>
where
    M: FromRusqliteRow + Clone,
    Rel: FromRusqliteRow + Clone + cratestack_sql::ModelPrimaryKey<RelPK>,
    RelPK: Clone + std::cmp::Eq + std::hash::Hash + IntoSqlValue,
{
    // Same shape as the sqlx implementation: collect distinct FK
    // values, side-load related rows via the runtime's IN-list path,
    // then merge by extracted primary key in memory.
    let mut fk_values: Vec<RelPK> = Vec::new();
    let mut seen: std::collections::HashSet<RelPK> = std::collections::HashSet::new();
    for parent in &parents {
        if let Some(fk) = (relation.parent_fk_extract)(parent)
            && seen.insert(fk.clone())
        {
            fk_values.push(fk);
        }
    }

    let by_pk: std::collections::HashMap<RelPK, Rel> = if fk_values.is_empty() {
        std::collections::HashMap::new()
    } else {
        let primary_key = relation.related_descriptor.primary_key;
        let mut probe = FindMany {
            runtime,
            descriptor: relation.related_descriptor,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };
        probe.filters.push(FilterExpr::from(cratestack_sql::Filter {
            column: primary_key,
            op: cratestack_sql::FilterOp::In,
            value: cratestack_sql::FilterValue::Many(
                fk_values
                    .iter()
                    .cloned()
                    .map(cratestack_sql::IntoSqlValue::into_sql_value)
                    .collect(),
            ),
        }));
        let related_rows = match conn {
            Some(conn) => probe.run_in_tx(conn)?,
            None => probe.run()?,
        };
        related_rows
            .into_iter()
            .map(|r| {
                let pk = cratestack_sql::ModelPrimaryKey::primary_key(&r);
                (pk, r)
            })
            .collect()
    };

    Ok(parents
        .into_iter()
        .map(|m| {
            let related = (relation.parent_fk_extract)(&m)
                .and_then(|fk| by_pk.get(&fk).cloned());
            (m, related)
        })
        .collect())
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

// ───── DeleteMany ──────────────────────────────────────────────────────────

pub struct DeleteMany<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> DeleteMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    /// Conditionally append a filter; `None` is a no-op. See
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

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) = render_delete_many(&dialect, self.descriptor, &self.filters);
        sql
    }

    pub fn run(self) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        if self.filters.is_empty() {
            return Err(RusqliteError::Validation(
                "delete_many requires at least one filter".to_owned(),
            ));
        }
        let dialect = SqliteDialect;
        let (sql, binds) = render_delete_many(&dialect, self.descriptor, &self.filters);
        self.runtime
            .with_connection(|conn| run_delete_many_returning::<M>(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        if self.filters.is_empty() {
            return Err(RusqliteError::Validation(
                "delete_many requires at least one filter".to_owned(),
            ));
        }
        let dialect = SqliteDialect;
        let (sql, binds) = render_delete_many(&dialect, self.descriptor, &self.filters);
        run_delete_many_returning::<M>(conn, &sql, &binds)
    }
}

fn run_delete_many_returning<M: FromRusqliteRow>(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: &[SqlValue],
) -> Result<BatchSummary, RusqliteError> {
    let mut stmt = conn.prepare(sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut count = 0usize;
    {
        let iter = stmt.query_map(params_from_iter(bind_iter), |row| {
            M::from_rusqlite_row(row).map(|_| ())
        })?;
        for item in iter {
            item?;
            count += 1;
        }
    }
    Ok(BatchSummary { total: count, ok: count, err: 0 })
}

// ───── Aggregate ───────────────────────────────────────────────────────────

pub struct Aggregate<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
}

impl<'a, M: 'static, PK: 'static> Aggregate<'a, M, PK> {
    pub fn count(self) -> AggregateCount<'a, M, PK> {
        AggregateCount {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
        }
    }

    pub fn sum<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Sum, column)
    }

    pub fn avg<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Avg, column)
    }

    pub fn min<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Min, column)
    }

    pub fn max<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Max, column)
    }
}

#[derive(Debug, Clone, Copy)]
enum AggregateOp {
    Sum,
    Avg,
    Min,
    Max,
}

impl AggregateOp {
    fn function_name(self) -> &'static str {
        match self {
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }
}

pub struct AggregateCount<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateCount<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    fn render(&self) -> (String, Vec<SqlValue>) {
        render_aggregate(
            self.descriptor,
            AggregateProjection::CountStar,
            &self.filters,
        )
    }

    pub fn run(self) -> Result<i64, RusqliteError> {
        let (sql, binds) = self.render();
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let value: i64 = stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
            Ok(value)
        })
    }

    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<i64, RusqliteError> {
        let (sql, binds) = self.render();
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let value: i64 = stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
        Ok(value)
    }
}

pub struct AggregateColumn<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    op: AggregateOp,
    column: &'static str,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateColumn<'a, M, PK> {
    fn new<C: cratestack_sql::IntoColumnName>(
        runtime: &'a RusqliteRuntime,
        descriptor: &'static ModelDescriptor<M, PK>,
        op: AggregateOp,
        column: C,
    ) -> Self {
        Self {
            runtime,
            descriptor,
            op,
            column: column.into_column_name(),
            filters: Vec::new(),
        }
    }

    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    fn render(&self) -> (String, Vec<SqlValue>) {
        render_aggregate(
            self.descriptor,
            AggregateProjection::Column {
                function: self.op.function_name(),
                column: self.column,
            },
            &self.filters,
        )
    }

    /// Run the aggregate. `T` is whatever `rusqlite::types::FromSql`-shaped
    /// scalar the call site wants — `i64` for `SUM(int)`, `f64` for
    /// `AVG(int)`, `chrono::DateTime` for `MIN(timestamp)`, etc.
    pub fn run<T>(self) -> Result<Option<T>, RusqliteError>
    where
        T: rusqlite::types::FromSql,
    {
        let (sql, binds) = self.render();
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let value: Option<T> =
                stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
            Ok(value)
        })
    }

    pub fn run_in_tx<T>(self, conn: &rusqlite::Connection) -> Result<Option<T>, RusqliteError>
    where
        T: rusqlite::types::FromSql,
    {
        let (sql, binds) = self.render();
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let value: Option<T> =
            stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
        Ok(value)
    }
}

enum AggregateProjection<'a> {
    CountStar,
    Column {
        function: &'static str,
        column: &'a str,
    },
}

fn render_aggregate<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    projection: AggregateProjection<'_>,
    filters: &[FilterExpr],
) -> (String, Vec<SqlValue>) {
    use std::fmt::Write;
    let dialect = SqliteDialect;
    let mut sql = String::from("SELECT ");
    match projection {
        AggregateProjection::CountStar => sql.push_str("COUNT(*)"),
        AggregateProjection::Column { function, column } => {
            let _ = write!(sql, "{function}({column})");
        }
    }
    let _ = write!(sql, " FROM {}", descriptor.table_name);

    let mut binds: Vec<SqlValue> = Vec::new();
    let mut bind_index = 1usize;
    let mut where_started = false;
    if let Some(soft_delete) = descriptor.soft_delete_column {
        let _ = write!(sql, " WHERE {soft_delete} IS NULL");
        where_started = true;
    }
    if !filters.is_empty() {
        sql.push_str(if where_started { " AND " } else { " WHERE " });
        let mut joined = false;
        for filter in filters {
            if joined {
                sql.push_str(" AND ");
            }
            crate::render::render_filter_expr(
                &dialect,
                filter,
                &mut sql,
                &mut binds,
                &mut bind_index,
            );
            joined = true;
        }
    }
    (sql, binds)
}

// ───── Column projection (.select) ─────────────────────────────────────────

pub struct ProjectedFindUnique<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    selected: Vec<&'static str>,
}

impl<'a, M: 'static, PK: 'static> ProjectedFindUnique<'a, M, PK>
where
    PK: IntoSqlValue + Clone,
{
    pub fn run(
        self,
    ) -> Result<Option<cratestack_sql::Projection<M>>, RusqliteError>
    where
        M: crate::FromPartialRusqliteRow,
    {
        let projection_sql = self.descriptor.select_projection_subset(&self.selected);
        let mut sql = format!(
            "SELECT {} FROM {} WHERE {} = ?1",
            projection_sql, self.descriptor.table_name, self.descriptor.primary_key,
        );
        if let Some(soft_delete) = self.descriptor.soft_delete_column {
            sql.push_str(&format!(" AND {soft_delete} IS NULL"));
        }
        sql.push_str(" LIMIT 1");
        let bind = self.id.clone().into_sql_value();
        let selected = self.selected;
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = std::iter::once(SqlValueParam(&bind));
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            if let Some(row) = rows.next()? {
                let value = M::from_partial_rusqlite_row(row, &selected)?;
                Ok(Some(cratestack_sql::Projection {
                    value,
                    selected,
                }))
            } else {
                Ok(None)
            }
        })
    }

    pub fn run_in_tx(
        self,
        conn: &rusqlite::Connection,
    ) -> Result<Option<cratestack_sql::Projection<M>>, RusqliteError>
    where
        M: crate::FromPartialRusqliteRow,
    {
        let projection_sql = self.descriptor.select_projection_subset(&self.selected);
        let mut sql = format!(
            "SELECT {} FROM {} WHERE {} = ?1",
            projection_sql, self.descriptor.table_name, self.descriptor.primary_key,
        );
        if let Some(soft_delete) = self.descriptor.soft_delete_column {
            sql.push_str(&format!(" AND {soft_delete} IS NULL"));
        }
        sql.push_str(" LIMIT 1");
        let bind = self.id.clone().into_sql_value();
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = std::iter::once(SqlValueParam(&bind));
        let mut rows = stmt.query(params_from_iter(bind_iter))?;
        if let Some(row) = rows.next()? {
            let value = M::from_partial_rusqlite_row(row, &self.selected)?;
            Ok(Some(cratestack_sql::Projection {
                value,
                selected: self.selected,
            }))
        } else {
            Ok(None)
        }
    }
}

impl<'a, M: 'static, PK: 'static> FindUnique<'a, M, PK>
where
    PK: IntoSqlValue + Clone,
{
    /// Restrict the SELECT to the named columns; see
    /// [`cratestack_sqlx::FindUnique::select`] for the shared
    /// caller-side contract. Returns `Option<Projection<M>>`.
    pub fn select<I, C>(self, columns: I) -> ProjectedFindUnique<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: cratestack_sql::IntoColumnName,
    {
        ProjectedFindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            selected: columns
                .into_iter()
                .map(cratestack_sql::IntoColumnName::into_column_name)
                .collect(),
        }
    }
}

pub struct ProjectedFindMany<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
    order_by: Vec<OrderClause>,
    limit: Option<i64>,
    offset: Option<i64>,
    selected: Vec<&'static str>,
}

impl<'a, M: 'static, PK: 'static> ProjectedFindMany<'a, M, PK> {
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

    pub fn run(self) -> Result<Vec<cratestack_sql::Projection<M>>, RusqliteError>
    where
        M: crate::FromPartialRusqliteRow,
    {
        // We reuse the regular `render_select` rendering but swap in
        // the subset projection. Easier than maintaining a parallel
        // render fn — the only delta is the projection list, and
        // build_select_with_projection inlines the descriptor's
        // subset projection in place of the full one.
        let dialect = SqliteDialect;
        let (sql, binds) = build_partial_select(
            &dialect,
            self.descriptor,
            &self.selected,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        let selected = self.selected;
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let rows = stmt
                .query_map(params_from_iter(bind_iter), |row| {
                    M::from_partial_rusqlite_row(row, &selected)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows
                .into_iter()
                .map(|value| cratestack_sql::Projection {
                    value,
                    selected: selected.clone(),
                })
                .collect())
        })
    }
}

impl<'a, M: 'static, PK: 'static> FindMany<'a, M, PK> {
    pub fn select<I, C>(self, columns: I) -> ProjectedFindMany<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: cratestack_sql::IntoColumnName,
    {
        ProjectedFindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: self.filters,
            order_by: self.order_by,
            limit: self.limit,
            offset: self.offset,
            selected: columns
                .into_iter()
                .map(cratestack_sql::IntoColumnName::into_column_name)
                .collect(),
        }
    }
}

fn build_partial_select<M, PK>(
    dialect: &dyn cratestack_sql::Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    selected: &[&'static str],
    filters: &[FilterExpr],
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
) -> (String, Vec<SqlValue>) {
    use std::fmt::Write;
    let mut sql = format!(
        "SELECT {} FROM {}",
        descriptor.select_projection_subset(selected),
        descriptor.table_name,
    );
    let mut binds: Vec<SqlValue> = Vec::new();
    let mut bind_index = 1usize;
    let mut where_sql = String::new();
    let mut wrote = false;
    if let Some(soft_delete) = descriptor.soft_delete_column {
        let _ = write!(&mut where_sql, "{soft_delete} IS NULL");
        wrote = true;
    }
    if !filters.is_empty() {
        if wrote {
            where_sql.push_str(" AND ");
        }
        let mut joined = false;
        for filter in filters {
            if joined {
                where_sql.push_str(" AND ");
            }
            crate::render::render_filter_expr(
                dialect,
                filter,
                &mut where_sql,
                &mut binds,
                &mut bind_index,
            );
            joined = true;
        }
    }
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }
    if !order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        for (idx, clause) in order_by.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            // Cheap inline rewrite of render_order_clause — it isn't
            // pub from render.rs and we only need the column-target
            // path here in practice. For relation-scalar order in a
            // projection we'd defer; for v1 of `.select(...)` plain
            // column ordering is the common case.
            use cratestack_sql::{OrderTarget, SortDirection};
            match &clause.target {
                OrderTarget::Column(column) => {
                    let direction = match clause.direction {
                        SortDirection::Asc => "ASC",
                        SortDirection::Desc => "DESC",
                    };
                    let nulls = match clause.null_order {
                        cratestack_sql::NullOrder::First => "NULLS FIRST",
                        cratestack_sql::NullOrder::Last => "NULLS LAST",
                    };
                    let _ = write!(&mut sql, "{column} {direction} {nulls}");
                }
                OrderTarget::RelationScalar { .. } => {
                    // Relation-scalar ordering on a projected query is
                    // a non-v1 shape — skip the clause silently rather
                    // than emit something that'd join the relation
                    // table while we're trying to keep the projection
                    // narrow.
                }
            }
        }
    }
    if let Some(limit_value) = limit {
        sql.push_str(" LIMIT ");
        dialect.write_placeholder(&mut sql, bind_index);
        bind_index += 1;
        binds.push(SqlValue::Int(limit_value));
    }
    if let Some(offset_value) = offset {
        sql.push_str(" OFFSET ");
        dialect.write_placeholder(&mut sql, bind_index);
        binds.push(SqlValue::Int(offset_value));
    }
    (sql, binds)
}
