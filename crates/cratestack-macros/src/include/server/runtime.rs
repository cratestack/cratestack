//! Runtime types emitted inside `pub mod cratestack_schema { ... }`:
//! `Cratestack` (the delegate hub), `BoundCratestack` (context-bound
//! view), `CratestackBuilder`, plus `schema_summary()`.
//!
//! Dispatches on `db` (cratestack#328): `db = Postgres` ([`postgres`])
//! keeps the pre-existing sqlx-backed shape byte-for-byte; `db = None`
//! ([`none`]) emits a genuinely different, database-free `Cratestack`
//! rather than threading an always-`None` `Option<PgPool>` through the
//! same type. See `none`'s module doc for the full rationale.

mod none;
mod postgres;
#[cfg(test)]
mod tests;

use super::super::parse::ServerDb;

pub(super) fn build_runtime_block(
    db: ServerDb,
    model_accessors: &[proc_macro2::TokenStream],
    bound_model_accessors: &[proc_macro2::TokenStream],
    view_accessors: &[proc_macro2::TokenStream],
    query_accessors: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    match db {
        ServerDb::Postgres => postgres::build_runtime_block(
            model_accessors,
            bound_model_accessors,
            view_accessors,
            query_accessors,
        ),
        // Zero models are guaranteed under `db = None` (cratestack#327's
        // datasource guard), so `model_accessors`/`bound_model_accessors`/
        // `view_accessors` are always empty here — the `none` variant
        // doesn't take them at all rather than accepting and discarding
        // dead parameters. `query_accessors` is empty for the same class of
        // reason but enforced by a guard of its own
        // (`super::query_guard`), because a schema with *no* `datasource`
        // block at all can pair `db = None` with real `query` blocks and
        // would otherwise reach here.
        ServerDb::None => none::build_runtime_block(),
    }
}
