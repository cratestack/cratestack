use std::fmt::Write;
use std::marker::PhantomData;

use cratestack_core::ModelEventKind;
use cratestack_policy::ReadPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelColumn {
    pub rust_name: &'static str,
    pub sql_name: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelDescriptor<M, PK> {
    pub schema_name: &'static str,
    pub table_name: &'static str,
    pub columns: &'static [ModelColumn],
    pub primary_key: &'static str,
    pub allowed_fields: &'static [&'static str],
    pub allowed_includes: &'static [&'static str],
    pub allowed_sorts: &'static [&'static str],
    pub read_allow_policies: &'static [ReadPolicy],
    pub read_deny_policies: &'static [ReadPolicy],
    pub detail_allow_policies: &'static [ReadPolicy],
    pub detail_deny_policies: &'static [ReadPolicy],
    pub create_allow_policies: &'static [ReadPolicy],
    pub create_deny_policies: &'static [ReadPolicy],
    pub update_allow_policies: &'static [ReadPolicy],
    pub update_deny_policies: &'static [ReadPolicy],
    pub delete_allow_policies: &'static [ReadPolicy],
    pub delete_deny_policies: &'static [ReadPolicy],
    pub create_defaults: &'static [CreateDefault],
    pub emitted_events: &'static [ModelEventKind],
    /// Column name of the optimistic-locking version field, set when the
    /// model declares an `@version` field. `None` for non-versioned models,
    /// which keeps update semantics unchanged.
    pub version_column: Option<&'static str>,
    /// `true` when the model declared `@@audit`. Mutations on audit-enabled
    /// models capture before/after snapshots and persist them into
    /// `cratestack_audit` inside the same transaction.
    pub audit_enabled: bool,
    /// SQL column names of fields declared `@pii`. The audit-log writer
    /// replaces these values with `"[redacted-pii]"` in the persisted JSON
    /// snapshots; a follow-up will extend the same redaction to error
    /// detail and tracing.
    pub pii_columns: &'static [&'static str],
    /// SQL column names of fields declared `@sensitive`. Redacted in audit
    /// snapshots as `"[redacted-sensitive]"`.
    pub sensitive_columns: &'static [&'static str],
    /// Column name for the soft-delete timestamp. When `Some`, DELETE
    /// operations become UPDATE-of-`deleted_at` and every SELECT through
    /// `push_scoped_conditions` filters out rows where the column is
    /// non-null. Defaults to `Some("deleted_at")` when `@@soft_delete` is
    /// declared.
    pub soft_delete_column: Option<&'static str>,
    /// Retention window in days for soft-deleted rows. The runtime does
    /// not auto-GC; banks run their own scheduled job that deletes rows
    /// where `deleted_at < NOW() - retention`. Surfaced here so the GC
    /// can read the policy from one place.
    pub retention_days: Option<u32>,
    _marker: PhantomData<fn() -> (M, PK)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateDefaultType {
    Bool,
    Int,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateDefault {
    pub column: &'static str,
    pub auth_field: &'static str,
    pub ty: CreateDefaultType,
    pub nullable: bool,
}

impl<M, PK> ModelDescriptor<M, PK> {
    pub const fn new(
        schema_name: &'static str,
        table_name: &'static str,
        columns: &'static [ModelColumn],
        primary_key: &'static str,
        allowed_fields: &'static [&'static str],
        allowed_includes: &'static [&'static str],
        allowed_sorts: &'static [&'static str],
        read_allow_policies: &'static [ReadPolicy],
        read_deny_policies: &'static [ReadPolicy],
        detail_allow_policies: &'static [ReadPolicy],
        detail_deny_policies: &'static [ReadPolicy],
        create_allow_policies: &'static [ReadPolicy],
        create_deny_policies: &'static [ReadPolicy],
        update_allow_policies: &'static [ReadPolicy],
        update_deny_policies: &'static [ReadPolicy],
        delete_allow_policies: &'static [ReadPolicy],
        delete_deny_policies: &'static [ReadPolicy],
        create_defaults: &'static [CreateDefault],
        emitted_events: &'static [ModelEventKind],
        version_column: Option<&'static str>,
        audit_enabled: bool,
        pii_columns: &'static [&'static str],
        sensitive_columns: &'static [&'static str],
        soft_delete_column: Option<&'static str>,
        retention_days: Option<u32>,
    ) -> Self {
        Self {
            schema_name,
            table_name,
            columns,
            primary_key,
            allowed_fields,
            allowed_includes,
            allowed_sorts,
            read_allow_policies,
            read_deny_policies,
            detail_allow_policies,
            detail_deny_policies,
            create_allow_policies,
            create_deny_policies,
            update_allow_policies,
            update_deny_policies,
            delete_allow_policies,
            delete_deny_policies,
            create_defaults,
            emitted_events,
            version_column,
            audit_enabled,
            pii_columns,
            sensitive_columns,
            soft_delete_column,
            retention_days,
            _marker: PhantomData,
        }
    }

    pub fn emits(&self, operation: ModelEventKind) -> bool {
        self.emitted_events.contains(&operation)
    }

    pub fn select_projection(&self) -> String {
        let mut sql = String::new();
        for (index, column) in self.columns.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let _ = write!(sql, "{} AS \"{}\"", column.sql_name, column.rust_name);
        }
        sql
    }
}
