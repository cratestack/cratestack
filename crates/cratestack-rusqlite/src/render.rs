//! SQLite SQL renderer.
//!
//! No policy support — on device the runtime is single-user and authorization
//! is not enforced at the storage layer. This makes the renderer noticeably
//! simpler than the cratestack-sqlx one: just filters, ordering, paging, and
//! the obvious INSERT/UPDATE/DELETE statements.
//!
//! Output is consumed by `rusqlite::Statement` with positional `?N`
//! placeholders. Bind ordering matches the order in which placeholders are
//! emitted into the SQL string.

mod coalesce;
mod delete;
mod delete_many;
mod filter;
mod insert;
mod json;
mod order;
mod relation;
mod select;
mod update;
mod update_many;
mod upsert;

#[cfg(test)]
mod tests_fixtures;
#[cfg(test)]
mod tests_mutate;
#[cfg(test)]
mod tests_predicates;
#[cfg(test)]
mod tests_select;
#[cfg(test)]
mod tests_upsert;

pub use delete::render_delete;
pub use delete_many::render_delete_many;
pub use insert::render_insert;
pub use select::{render_select, render_select_by_pk};
pub use update::render_update;
pub use update_many::render_update_many;
pub use upsert::{render_upsert, render_upsert_with_conflict};

pub(crate) use filter::render_filter_expr;
