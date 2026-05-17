//! `ModelDelegate` and its scoped (`bind(ctx)`-bound) sibling, plus
//! the per-operation scoped builder wrappers. The unscoped delegate
//! hands out `FindMany`/`CreateRecord`/etc. directly; the scoped
//! delegate captures a `CoolContext` once at `bind` time and threads
//! it into every `.run()` so call sites stay terse.

mod model;
mod model_authorize;
mod model_batch;
mod scoped;
mod scoped_aggregate;
mod scoped_batch;
mod scoped_delete;
mod scoped_find_many;
mod scoped_find_many_projected;
mod scoped_find_many_with;
mod scoped_find_unique;
mod scoped_update_many;
mod scoped_writes;

pub use model::ModelDelegate;
pub use scoped::ScopedModelDelegate;
pub use scoped_aggregate::{ScopedAggregate, ScopedAggregateColumn, ScopedAggregateCount};
pub use scoped_batch::{
    ScopedBatchCreate, ScopedBatchDelete, ScopedBatchGet, ScopedBatchUpdate, ScopedBatchUpsert,
};
pub use scoped_delete::{ScopedDeleteMany, ScopedDeleteRecord};
pub use scoped_find_many::ScopedFindMany;
pub use scoped_find_many_projected::ScopedProjectedFindMany;
pub use scoped_find_many_with::ScopedFindManyWith;
pub use scoped_find_unique::{ScopedFindUnique, ScopedProjectedFindUnique};
pub use scoped_update_many::{ScopedUpdateMany, ScopedUpdateManySet};
pub use scoped_writes::{
    ScopedCreateRecord, ScopedUpdateRecord, ScopedUpdateRecordSet, ScopedUpsertRecord,
};
