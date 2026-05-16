//! `ModelDelegate` — entry point that hands out per-operation builders.

use cratestack_sql::{ConflictTarget, IntoSqlValue, ModelDescriptor};

use crate::RusqliteRuntime;

use super::aggregate::Aggregate;
use super::create::CreateRecord;
use super::delete::DeleteRecord;
use super::delete_many::DeleteMany;
use super::find_many::FindMany;
use super::find_unique::FindUnique;
use super::update::UpdateRecord;
use super::update_many::UpdateMany;
use super::upsert::UpsertRecord;

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
