//! `ReadSource` / `WriteSource` impls for
//! [`ModelDescriptor`](super::ModelDescriptor). Pulled into its own
//! file to keep `descriptor/mod.rs` under the 200-LoC ceiling.
//!
//! The impls are pure delegation — each trait method returns the
//! identically-named `ModelDescriptor` field. They exist so the
//! upcoming view-aware read builders can take either
//! `&ModelDescriptor<M, PK>` or `&ViewDescriptor<V, PK>` through a
//! shared trait bound without forcing the existing model-specific
//! call sites to change at the same time.

use cratestack_core::ModelEventKind;
use cratestack_policy::ReadPolicy;

use super::{CreateDefault, ModelColumn, ModelDescriptor, ReadSource, WriteSource};

impl<M, PK> ReadSource<M, PK> for ModelDescriptor<M, PK> {
    fn schema_name(&self) -> &'static str {
        self.schema_name
    }
    fn table_name(&self) -> &'static str {
        self.table_name
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
        self.allowed_includes
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
        self.soft_delete_column
    }
    fn select_projection(&self) -> String {
        ModelDescriptor::select_projection(self)
    }
    fn select_projection_subset(&self, columns: &[&str]) -> String {
        ModelDescriptor::select_projection_subset(self, columns)
    }
}

impl<M, PK> WriteSource<M, PK> for ModelDescriptor<M, PK> {
    fn create_allow_policies(&self) -> &'static [ReadPolicy] {
        self.create_allow_policies
    }
    fn create_deny_policies(&self) -> &'static [ReadPolicy] {
        self.create_deny_policies
    }
    fn update_allow_policies(&self) -> &'static [ReadPolicy] {
        self.update_allow_policies
    }
    fn update_deny_policies(&self) -> &'static [ReadPolicy] {
        self.update_deny_policies
    }
    fn delete_allow_policies(&self) -> &'static [ReadPolicy] {
        self.delete_allow_policies
    }
    fn delete_deny_policies(&self) -> &'static [ReadPolicy] {
        self.delete_deny_policies
    }
    fn create_defaults(&self) -> &'static [CreateDefault] {
        self.create_defaults
    }
    fn emitted_events(&self) -> &'static [ModelEventKind] {
        self.emitted_events
    }
    fn version_column(&self) -> Option<&'static str> {
        self.version_column
    }
    fn audit_enabled(&self) -> bool {
        self.audit_enabled
    }
    fn pii_columns(&self) -> &'static [&'static str] {
        self.pii_columns
    }
    fn sensitive_columns(&self) -> &'static [&'static str] {
        self.sensitive_columns
    }
    fn retention_days(&self) -> Option<u32> {
        self.retention_days
    }
    fn upsert_update_columns(&self) -> &'static [&'static str] {
        self.upsert_update_columns
    }
}
