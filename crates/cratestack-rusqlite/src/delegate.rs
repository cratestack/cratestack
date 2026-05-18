//! Per-model ORM delegate — the sync mirror of `cratestack-sqlx::ModelDelegate`.

mod aggregate;
mod aggregate_column;
mod aggregate_count;
mod create;
mod delete;
mod delete_many;
mod find_many;
mod find_many_with;
mod find_unique;
mod model;
mod projected_find_many;
mod projected_find_unique;
mod projected_select;
mod support;
mod update;
mod update_many;
mod upsert;
mod view;

pub use aggregate::Aggregate;
pub use aggregate_column::AggregateColumn;
pub use aggregate_count::AggregateCount;
pub use create::CreateRecord;
pub use delete::DeleteRecord;
pub use delete_many::DeleteMany;
pub use find_many::FindMany;
pub use find_many_with::FindManyWith;
pub use find_unique::FindUnique;
pub use model::ModelDelegate;
pub use projected_find_many::ProjectedFindMany;
pub use projected_find_unique::ProjectedFindUnique;
pub use update::{UpdateRecord, UpdateRecordSet};
pub use update_many::{UpdateMany, UpdateManySet};
pub use upsert::UpsertRecord;
pub use view::{ViewDelegate, ViewDelegateNoUnique};
