//! `ScopedModelDelegate` — context-bound view of a `ModelDelegate`.
//! Every method here returns a scoped builder that thread its captured
//! `CoolContext` through to the underlying unbound builder's `.run()`.

use cratestack_core::CoolContext;

use crate::{BatchUpdateItem, ModelDescriptor};

use super::model::ModelDelegate;
use super::scoped_aggregate::ScopedAggregate;
use super::scoped_batch::{
    ScopedBatchCreate, ScopedBatchDelete, ScopedBatchGet, ScopedBatchUpdate, ScopedBatchUpsert,
};
use super::scoped_delete::{ScopedDeleteMany, ScopedDeleteRecord};
use super::scoped_find_many::ScopedFindMany;
use super::scoped_find_unique::ScopedFindUnique;
use super::scoped_update_many::ScopedUpdateMany;
use super::scoped_writes::{ScopedCreateRecord, ScopedUpdateRecord, ScopedUpsertRecord};

#[derive(Debug, Clone)]
pub struct ScopedModelDelegate<'a, M: 'static, PK: 'static> {
    pub(super) delegate: ModelDelegate<'a, M, PK>,
    pub(super) ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedModelDelegate<'a, M, PK> {
    pub(super) fn new(delegate: ModelDelegate<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { delegate, ctx }
    }

    pub fn descriptor(&self) -> &'static ModelDescriptor<M, PK> {
        self.delegate.descriptor()
    }

    pub fn context(&self) -> &CoolContext {
        &self.ctx
    }

    pub fn find_many(&self) -> ScopedFindMany<'a, M, PK> {
        ScopedFindMany::new(self.delegate.find_many(), self.ctx.clone())
    }

    pub fn find_unique(&self, id: PK) -> ScopedFindUnique<'a, M, PK> {
        ScopedFindUnique::new(self.delegate.find_unique(id), self.ctx.clone())
    }

    pub fn create<I>(&self, input: I) -> ScopedCreateRecord<'a, M, PK, I> {
        ScopedCreateRecord::new(self.delegate.create(input), self.ctx.clone())
    }

    pub fn upsert<I>(&self, input: I) -> ScopedUpsertRecord<'a, M, PK, I> {
        ScopedUpsertRecord::new(self.delegate.upsert(input), self.ctx.clone())
    }

    pub fn update(&self, id: PK) -> ScopedUpdateRecord<'a, M, PK> {
        ScopedUpdateRecord::new(self.delegate.update(id), self.ctx.clone())
    }

    pub fn update_many(&self) -> ScopedUpdateMany<'a, M, PK> {
        ScopedUpdateMany::new(self.delegate.update_many(), self.ctx.clone())
    }

    pub fn delete(&self, id: PK) -> ScopedDeleteRecord<'a, M, PK> {
        ScopedDeleteRecord::new(self.delegate.delete(id), self.ctx.clone())
    }

    pub fn delete_many(&self) -> ScopedDeleteMany<'a, M, PK> {
        ScopedDeleteMany::new(self.delegate.delete_many(), self.ctx.clone())
    }

    pub fn aggregate(&self) -> ScopedAggregate<'a, M, PK> {
        ScopedAggregate::new(self.delegate.aggregate(), self.ctx.clone())
    }

    pub fn batch_get(&self, ids: Vec<PK>) -> ScopedBatchGet<'a, M, PK> {
        ScopedBatchGet::new(self.delegate.batch_get(ids), self.ctx.clone())
    }

    pub fn batch_create<I>(&self, inputs: Vec<I>) -> ScopedBatchCreate<'a, M, PK, I> {
        ScopedBatchCreate::new(self.delegate.batch_create(inputs), self.ctx.clone())
    }

    pub fn batch_update<I>(
        &self,
        items: Vec<BatchUpdateItem<PK, I>>,
    ) -> ScopedBatchUpdate<'a, M, PK, I> {
        ScopedBatchUpdate::new(self.delegate.batch_update(items), self.ctx.clone())
    }

    pub fn batch_delete(&self, ids: Vec<PK>) -> ScopedBatchDelete<'a, M, PK> {
        ScopedBatchDelete::new(self.delegate.batch_delete(ids), self.ctx.clone())
    }

    pub fn batch_upsert<I>(&self, inputs: Vec<I>) -> ScopedBatchUpsert<'a, M, PK, I> {
        ScopedBatchUpsert::new(self.delegate.batch_upsert(inputs), self.ctx.clone())
    }
}
