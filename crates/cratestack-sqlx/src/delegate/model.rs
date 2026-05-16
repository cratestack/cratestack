//! `ModelDelegate` — the per-model entry point handed out by the
//! generated `Cratestack::<model>()` accessor. Hosts the unbound (no
//! `CoolContext`) builders for every CRUD/aggregate primitive. Batch
//! and authorize methods live in [`super::model_batch`] and
//! [`super::model_authorize`] respectively.

use cratestack_core::CoolContext;

use crate::{
    Aggregate, CreateRecord, DeleteMany, DeleteRecord, FindMany, FindUnique, ModelDescriptor,
    SqlxRuntime, UpdateMany, UpdateRecord, UpsertRecord,
};

use super::scoped::ScopedModelDelegate;

#[derive(Debug, Clone, Copy)]
pub struct ModelDelegate<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a SqlxRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
}

impl<'a, M: 'static, PK: 'static> ModelDelegate<'a, M, PK> {
    pub fn new(runtime: &'a SqlxRuntime, descriptor: &'static ModelDescriptor<M, PK>) -> Self {
        Self { runtime, descriptor }
    }

    pub fn descriptor(&self) -> &'static ModelDescriptor<M, PK> {
        self.descriptor
    }

    pub fn bind(self, ctx: CoolContext) -> ScopedModelDelegate<'a, M, PK> {
        ScopedModelDelegate::new(self, ctx)
    }

    pub fn find_many(&self) -> FindMany<'a, M, PK> {
        FindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            for_update: false,
        }
    }

    pub fn find_unique(&self, id: PK) -> FindUnique<'a, M, PK> {
        FindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
            for_update: false,
            policy_kind: crate::query::ReadPolicyKind::Detail,
        }
    }

    pub fn create<I>(&self, input: I) -> CreateRecord<'a, M, PK, I> {
        CreateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input,
        }
    }

    /// Insert-or-update on primary-key conflict. Available only on
    /// models whose `@id` field is client-supplied (no `@default(...)`);
    /// attempting to call this on a model with a server-generated PK
    /// is a compile error.
    pub fn upsert<I>(&self, input: I) -> UpsertRecord<'a, M, PK, I> {
        UpsertRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input,
            conflict_target: cratestack_sql::ConflictTarget::PrimaryKey,
        }
    }

    pub fn update(&self, id: PK) -> UpdateRecord<'a, M, PK> {
        UpdateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    /// Bulk UPDATE by predicate. Refuses to run without at least one
    /// filter — table-wide bulk updates are a footgun that should be
    /// written in raw SQL.
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

    /// Side-effect-free aggregate read. Returns a builder that
    /// branches into `.count()` / `.sum(col)` / `.avg(col)` /
    /// `.min(col)` / `.max(col)`. Aggregates apply the read policy
    /// AND soft-delete column so the result always describes rows the
    /// caller could retrieve via `find_many`.
    pub fn aggregate(&self) -> Aggregate<'a, M, PK> {
        Aggregate {
            runtime: self.runtime,
            descriptor: self.descriptor,
        }
    }

    /// Bulk DELETE by predicate. Mirrors `update_many`: applies the
    /// delete policy and soft-delete column (if any), fans audit +
    /// outbox out per-row via RETURNING, refuses to run without at
    /// least one filter.
    pub fn delete_many(&self) -> DeleteMany<'a, M, PK> {
        DeleteMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
        }
    }
}
