//! Read primitives — `find_many`, `find_unique`, aggregates, and
//! their projected (`.select(...)`) variants. Every read flows through
//! [`crate::query::support::push_scoped_conditions`] so soft-delete +
//! read policy apply uniformly.

mod aggregate;
mod aggregate_column;
mod aggregate_count;
mod find_many;
mod find_many_preview;
mod find_many_with;
mod find_unique;
mod projected_find_many;
mod projected_find_unique;
mod side_load;

pub use aggregate::Aggregate;
pub use aggregate_column::AggregateColumn;
pub use aggregate_count::AggregateCount;
pub use find_many::FindMany;
pub use find_many_with::FindManyWith;
pub use find_unique::FindUnique;
pub use projected_find_many::ProjectedFindMany;
pub use projected_find_unique::ProjectedFindUnique;
