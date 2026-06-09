//! `ViewDescriptor` — the runtime sibling of [`super::ModelDescriptor`]
//! for `view` blocks (ADR-0003).
//!
//! Views are read-only, so this descriptor implements only
//! [`super::ReadSource`] — never [`super::WriteSource`]. The type
//! system enforces read-only-ness: it is impossible to pass a
//! `ViewDescriptor` to a write-path builder because the bound doesn't
//! hold.
//!
//! The shape is intentionally narrower than `ModelDescriptor`. Views
//! carry no:
//!
//! - create / update / delete policy slots
//! - create defaults, emitted events, audit / retention / version /
//!   PII / sensitive metadata
//! - upsert-overwrite column list
//! - soft-delete column (the view's SQL body is responsible for
//!   filtering soft-deleted source rows — see ADR §"Delegate split")
//! - relation includes (relation-follow off a view is deferred to a
//!   future ADR)
//!
//! Extra fields specific to views:
//!
//! - `is_materialized` — `true` for `@@materialized` views. Picked up
//!   by the macro to emit a `refresh()` method on the generated
//!   delegate (server-only).
//! - `source_tables` — the names of source tables / views the body
//!   reads from. Carried so the migration diff engine can order
//!   `CREATE VIEW` after its source `CREATE TABLE` and `DROP VIEW`
//!   before its source `DROP TABLE`.

use std::marker::PhantomData;

use cratestack_policy::ReadPolicy;

use super::{ModelColumn, ReadSource};

#[derive(Debug, Clone, Copy)]
pub struct ViewDescriptor<V, PK> {
    pub schema_name: &'static str,
    pub view_name: &'static str,
    pub columns: &'static [ModelColumn],
    /// SQL column name of the view's primary key. Empty string when
    /// the view was declared `@@no_unique` — in that case the macro
    /// also omits `find_unique` on the generated delegate.
    pub primary_key: &'static str,
    pub allowed_fields: &'static [&'static str],
    pub allowed_sorts: &'static [&'static str],
    pub read_allow_policies: &'static [ReadPolicy],
    pub read_deny_policies: &'static [ReadPolicy],
    pub detail_allow_policies: &'static [ReadPolicy],
    pub detail_deny_policies: &'static [ReadPolicy],
    /// `true` when the view was declared `@@materialized`. Embedded
    /// builds reject this at macro expansion time (SQLite has no
    /// materialized views); server builds emit a `refresh()` method.
    pub is_materialized: bool,
    /// Names of the models / views the SQL body reads from. Drives
    /// migration ordering — `CREATE VIEW` lands after its sources,
    /// `DROP VIEW` lands before them. Populated from the `from M, N`
    /// declaration on the schema, not parsed out of the SQL body.
    pub source_tables: &'static [&'static str],
    _marker: PhantomData<fn() -> (V, PK)>,
}

impl<V, PK> ViewDescriptor<V, PK> {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        schema_name: &'static str,
        view_name: &'static str,
        columns: &'static [ModelColumn],
        primary_key: &'static str,
        allowed_fields: &'static [&'static str],
        allowed_sorts: &'static [&'static str],
        read_allow_policies: &'static [ReadPolicy],
        read_deny_policies: &'static [ReadPolicy],
        detail_allow_policies: &'static [ReadPolicy],
        detail_deny_policies: &'static [ReadPolicy],
        is_materialized: bool,
        source_tables: &'static [&'static str],
    ) -> Self {
        Self {
            schema_name,
            view_name,
            columns,
            primary_key,
            allowed_fields,
            allowed_sorts,
            read_allow_policies,
            read_deny_policies,
            detail_allow_policies,
            detail_deny_policies,
            is_materialized,
            source_tables,
            _marker: PhantomData,
        }
    }
}

impl<V, PK> ReadSource<V, PK> for ViewDescriptor<V, PK> {
    fn schema_name(&self) -> &'static str {
        self.schema_name
    }
    fn table_name(&self) -> &'static str {
        // For views the "table" the read builder selects from is the
        // view's SQL identifier — sqlx and rusqlite quote it the same
        // way they would a real table.
        self.view_name
    }
    fn columns(&self) -> &'static [ModelColumn] {
        self.columns
    }
    fn primary_key(&self) -> &'static str {
        self.primary_key
    }
    fn allowed_fields(&self) -> &'static [&'static str] {
        self.allowed_fields
    }
    fn allowed_includes(&self) -> &'static [&'static str] {
        // Relation-follow off views is deferred (ADR-0003 §"Deferred").
        &[]
    }
    fn allowed_sorts(&self) -> &'static [&'static str] {
        self.allowed_sorts
    }
    fn read_allow_policies(&self) -> &'static [ReadPolicy] {
        self.read_allow_policies
    }
    fn read_deny_policies(&self) -> &'static [ReadPolicy] {
        self.read_deny_policies
    }
    fn detail_allow_policies(&self) -> &'static [ReadPolicy] {
        self.detail_allow_policies
    }
    fn detail_deny_policies(&self) -> &'static [ReadPolicy] {
        self.detail_deny_policies
    }
    fn soft_delete_column(&self) -> Option<&'static str> {
        // Views never carry soft-delete state — the source models do.
        // The view's SQL body is responsible for filtering soft-
        // deleted rows out of its projection.
        None
    }
    // `select_projection` / `select_projection_subset` use the trait's
    // default impls (they iterate `self.columns()`), which match
    // `ModelDescriptor`'s behavior exactly.
}
