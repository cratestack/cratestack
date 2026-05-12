use std::fmt::Write;
use std::marker::PhantomData;

use cratestack_core::ModelEventKind;
use cratestack_policy::ReadPolicy;

/// Mapping between a model's Rust field name and its SQL column name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelColumn {
    pub rust_name: &'static str,
    pub sql_name: &'static str,
}

/// Table identity — where the rows live and how the primary key is named.
/// Used by every render path; the rest of the descriptor describes
/// orthogonal concerns (authorization, audit, lifecycle).
#[derive(Debug, Clone, Copy)]
pub struct TableMeta {
    pub schema_name: &'static str,
    pub table_name: &'static str,
    pub columns: &'static [ModelColumn],
    pub primary_key: &'static str,
}

/// Surfaceable query shape — the fields, relations, and sort keys the
/// transport layer is willing to honour. Generated from
/// `@allow_field` / `@allow_include` / `@allow_sort` attributes; backends
/// don't use them directly, the macro-generated handlers consult them
/// when validating incoming requests.
#[derive(Debug, Clone, Copy)]
pub struct QueryCapabilities {
    pub allowed_fields: &'static [&'static str],
    pub allowed_includes: &'static [&'static str],
    pub allowed_sorts: &'static [&'static str],
}

/// Per-action allow/deny policy slices, baked at compile time from the
/// `@@allow` / `@@deny` model attributes. Backends evaluate the relevant
/// pair on every SELECT/INSERT/UPDATE/DELETE.
#[derive(Debug, Clone, Copy)]
pub struct AuthPolicies {
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
}

/// Audit-log configuration. `enabled` controls whether mutations write
/// `before`/`after` snapshots into `cratestack_audit`; the two column
/// slices drive the snapshot redactor in `cratestack_sqlx::audit`.
#[derive(Debug, Clone, Copy)]
pub struct AuditConfig {
    /// `true` when the model declared `@@audit`. Mutations on audit-enabled
    /// models capture before/after snapshots and persist them into
    /// `cratestack_audit` inside the same transaction.
    pub audit_enabled: bool,
    /// SQL column names of fields declared `@pii`. The audit-log writer
    /// replaces these values with `"[redacted-pii]"` in the persisted JSON
    /// snapshots.
    pub pii_columns: &'static [&'static str],
    /// SQL column names of fields declared `@sensitive`. Redacted in audit
    /// snapshots as `"[redacted-sensitive]"`.
    pub sensitive_columns: &'static [&'static str],
}

/// Per-row lifecycle hooks: optimistic locking, soft-delete, retention,
/// event emission, and create-time defaults.
#[derive(Debug, Clone, Copy)]
pub struct LifecycleConfig {
    pub create_defaults: &'static [CreateDefault],
    pub emitted_events: &'static [ModelEventKind],
    /// Column name of the optimistic-locking version field, set when the
    /// model declares an `@version` field. `None` for non-versioned models.
    pub version_column: Option<&'static str>,
    /// Column name for the soft-delete timestamp. When `Some`, DELETE
    /// operations become UPDATE-of-`deleted_at` and every SELECT through
    /// the scoped query filters out rows where the column is non-null.
    /// Defaults to `Some("deleted_at")` when `@@soft_delete` is declared.
    pub soft_delete_column: Option<&'static str>,
    /// Retention window in days for soft-deleted rows. The runtime does
    /// not auto-GC; banks run their own scheduled job that deletes rows
    /// where `deleted_at < NOW() - retention`. Surfaced here so the GC
    /// can read the policy from one place.
    pub retention_days: Option<u32>,
}

/// Compile-time schema metadata for a single model.
///
/// Previously a single 25-field struct mixing table identity, query
/// capabilities, authorization, audit, and lifecycle concerns into one
/// constructor signature. The fields are now grouped into sub-structs so
/// that adding (say) an encryption-config slice doesn't require threading
/// new parameters through every caller.
///
/// The descriptor remains `Copy` and trivially-cloneable; macro-emitted
/// `const` instances live in static memory.
#[derive(Debug, Clone, Copy)]
pub struct ModelDescriptor<M, PK> {
    pub table: TableMeta,
    pub query: QueryCapabilities,
    pub auth: AuthPolicies,
    pub audit: AuditConfig,
    pub lifecycle: LifecycleConfig,
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
    /// Construct a descriptor from its grouped configuration. All sub-
    /// structs are `Copy`; macro-emitted descriptors build them inline.
    pub const fn new(
        table: TableMeta,
        query: QueryCapabilities,
        auth: AuthPolicies,
        audit: AuditConfig,
        lifecycle: LifecycleConfig,
    ) -> Self {
        Self {
            table,
            query,
            auth,
            audit,
            lifecycle,
            _marker: PhantomData,
        }
    }

    /// Convenience accessor for the model's logical name (the Rust ident
    /// from the schema). Equivalent to `self.table.schema_name`.
    pub const fn schema_name(&self) -> &'static str {
        self.table.schema_name
    }

    /// Convenience accessor for the SQL table name.
    pub const fn table_name(&self) -> &'static str {
        self.table.table_name
    }

    /// Convenience accessor for the primary-key column name.
    pub const fn primary_key(&self) -> &'static str {
        self.table.primary_key
    }

    /// Convenience accessor for the column list.
    pub const fn columns(&self) -> &'static [ModelColumn] {
        self.table.columns
    }

    /// Soft-delete column, if the model declared `@@soft_delete`.
    pub const fn soft_delete_column(&self) -> Option<&'static str> {
        self.lifecycle.soft_delete_column
    }

    /// Optimistic-locking version column, if the model declared `@version`.
    pub const fn version_column(&self) -> Option<&'static str> {
        self.lifecycle.version_column
    }

    /// Whether the model emits the given event kind.
    pub fn emits(&self, operation: ModelEventKind) -> bool {
        self.lifecycle.emitted_events.contains(&operation)
    }

    /// Render the projection list — `<sql_name> AS "<rust_name>", …` —
    /// reused by every SELECT/INSERT/UPDATE statement.
    pub fn select_projection(&self) -> String {
        let mut sql = String::new();
        for (index, column) in self.table.columns.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let _ = write!(sql, "{} AS \"{}\"", column.sql_name, column.rust_name);
        }
        sql
    }
}
