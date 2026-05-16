//! `ModelDelegate` batch entry points (`batch_get`, `batch_create`,
//! `batch_update`, `batch_delete`, `batch_upsert`).

use crate::{BatchCreate, BatchDelete, BatchGet, BatchUpdate, BatchUpdateItem, BatchUpsert};

use super::model::ModelDelegate;

impl<'a, M: 'static, PK: 'static> ModelDelegate<'a, M, PK> {
    /// Fetch many rows by primary key in a single round-trip; missing
    /// rows surface as per-item `NotFound` in the envelope rather
    /// than aborting.
    pub fn batch_get(&self, ids: Vec<PK>) -> BatchGet<'a, M, PK> {
        BatchGet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            ids,
        }
    }

    /// Insert many rows in one outer transaction; each input runs
    /// under a nested SAVEPOINT, so a per-item failure (validation,
    /// policy, unique conflict) doesn't take down the rest of the
    /// batch.
    pub fn batch_create<I>(&self, inputs: Vec<I>) -> BatchCreate<'a, M, PK, I> {
        BatchCreate {
            runtime: self.runtime,
            descriptor: self.descriptor,
            inputs,
        }
    }

    /// Update many rows in one outer transaction with per-item
    /// patches and optional `if_match` versions. Per-item failures
    /// roll back at the savepoint; successful items commit together.
    pub fn batch_update<I>(
        &self,
        items: Vec<BatchUpdateItem<PK, I>>,
    ) -> BatchUpdate<'a, M, PK, I> {
        BatchUpdate {
            runtime: self.runtime,
            descriptor: self.descriptor,
            items,
        }
    }

    /// Delete many rows by primary key in a single statement; rows
    /// that don't exist (or that policy hid) surface as per-item
    /// `NotFound`.
    pub fn batch_delete(&self, ids: Vec<PK>) -> BatchDelete<'a, M, PK> {
        BatchDelete {
            runtime: self.runtime,
            descriptor: self.descriptor,
            ids,
        }
    }

    /// Insert-or-update many rows in one outer transaction with
    /// per-item savepoints. Eligible only for models whose `@id` is
    /// client-supplied — same compile-time gate as the single-row
    /// `.upsert(...)`.
    pub fn batch_upsert<I>(&self, inputs: Vec<I>) -> BatchUpsert<'a, M, PK, I> {
        BatchUpsert {
            runtime: self.runtime,
            descriptor: self.descriptor,
            inputs,
        }
    }
}
