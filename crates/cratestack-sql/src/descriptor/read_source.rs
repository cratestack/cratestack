//! Abstract read/write source traits shared between
//! [`ModelDescriptor`](super::ModelDescriptor) and
//! [`ViewDescriptor`](super::ViewDescriptor).
//!
//! These traits exist so the read-builder family
//! (`FindMany` / `FindUnique` / `Aggregate` / `push_scoped_conditions`)
//! can take *either* descriptor without the macro having to duplicate
//! the entire query surface for views. They're additive — the
//! existing builders still take `&'static ModelDescriptor<M, PK>`
//! today; the genericization to `&'static dyn ReadSource<M, PK>` (or
//! a blanket `D: ReadSource<M, PK>` bound) lands in a follow-up PR
//! once the trait shape has settled.
//!
//! **Why two traits?** [`ReadSource`] captures everything a read
//! builder needs (table / view name, columns, primary key, soft-delete
//! gate, read/detail policies, projection emission). [`WriteSource`]
//! extends it with the create/update/delete-only state — defaults,
//! audit, retention, versioning, upsert columns, write policy slots.
//!
//! Views deliberately do not implement `WriteSource`, so the macro
//! cannot accidentally wire a view through a write builder. The
//! read-only-ness guarantee for views is enforced at the type level.

use cratestack_core::ModelEventKind;
use cratestack_policy::ReadPolicy;

use super::{CreateDefault, ModelColumn};

/// Anything a read-path query builder needs to plan and emit SQL.
///
/// `M` is the Rust struct deserialized from a row; `PK` is the
/// primary-key Rust type. Both descriptors and the read builders are
/// generic over the same `(M, PK)` pair so the bounds line up.
///
/// `Send + Sync` are required so that `&'static dyn ReadSource<M, PK>`
/// is `Send`. Axum handler futures capture the trait object across
/// `await` points; without these bounds those futures stop being
/// `Send`, which makes them unusable as Axum handlers. Both
/// first-party impls (`ModelDescriptor`, `ViewDescriptor`) are
/// trivially `Send + Sync` — every field is either a `&'static`
/// reference to a primitive slice or a `PhantomData<fn() -> _>`, all
/// of which are themselves `Send + Sync` regardless of `M` / `PK`.
pub trait ReadSource<M, PK>: Send + Sync {
    /// Logical schema name the model / view lives under. Currently
    /// always the dataset schema declared in `datasource db { ... }`;
    /// kept on the trait so future per-source schemas (e.g. analytics
    /// views in a dedicated schema) are a non-breaking change.
    fn schema_name(&self) -> &'static str;

    /// SQL identifier of the table *or* view this source reads from.
    /// Both backends quote it verbatim when constructing `FROM`
    /// clauses.
    fn table_name(&self) -> &'static str;

    /// All projectable columns, ordered as the descriptor declares
    /// them. The read builder relies on this order when binding row
    /// decoders.
    fn columns(&self) -> &'static [ModelColumn];

    /// SQL column name of the primary key. For views declared with
    /// `@@no_unique` (ADR-0003 §"Schema surface") this is the empty
    /// string — `find_unique` is not emitted on the delegate so the
    /// builder never reads this slot.
    fn primary_key(&self) -> &'static str;

    /// Names accepted in `where = { <name>: <op> }` filter payloads
    /// — the same allow-list the model uses for read-policy scoping.
    fn allowed_fields(&self) -> &'static [&'static str];

    /// Names accepted in `include = { <name>: ... }` payloads. Empty
    /// on views in v1 (relation-follow off a view is out of scope —
    /// see ADR-0003 "Deferred").
    fn allowed_includes(&self) -> &'static [&'static str];

    /// Names accepted in `orderBy = [ <name>, ... ]` payloads.
    fn allowed_sorts(&self) -> &'static [&'static str];

    /// `@@allow("read", ...)` policy literals for the list / search
    /// shape (returns one row per matching record).
    fn read_allow_policies(&self) -> &'static [ReadPolicy];

    /// `@@deny("read", ...)` policy literals for the list shape.
    fn read_deny_policies(&self) -> &'static [ReadPolicy];

    /// `@@allow("read", ...)` policy literals for the detail shape
    /// (`find_unique` — returns at most one record). Models can carry
    /// stricter detail policies than list ones; views inherit a
    /// single set declared via `@@allow("read", ...)` on the view
    /// itself.
    fn detail_allow_policies(&self) -> &'static [ReadPolicy];

    /// `@@deny("read", ...)` policy literals for the detail shape.
    fn detail_deny_policies(&self) -> &'static [ReadPolicy];

    /// Soft-delete sentinel column name. `None` on views (and on
    /// models without `@@soft_delete`), in which case the read
    /// builder skips the `<col> IS NULL` predicate it would otherwise
    /// inject.
    fn soft_delete_column(&self) -> Option<&'static str>;

    /// Returns the `<col> AS "<alias>", ...` projection list the
    /// builder splices into `SELECT`. The default impl delegates to
    /// [`Self::columns`] so any descriptor that just stores a column
    /// list gets a working projection for free.
    fn select_projection(&self) -> String {
        use std::fmt::Write;
        let mut sql = String::new();
        for (index, column) in self.columns().iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let _ = write!(sql, "{} AS \"{}\"", column.sql_name, column.rust_name);
        }
        sql
    }

    /// Like [`Self::select_projection`] but emits only the named
    /// columns. Unknown names are silently dropped — same contract
    /// as [`super::ModelDescriptor::select_projection_subset`].
    fn select_projection_subset(&self, columns: &[&str]) -> String {
        use std::fmt::Write;
        let mut sql = String::new();
        let mut emitted = false;
        for column in self.columns().iter() {
            if columns.iter().any(|name| *name == column.sql_name) {
                if emitted {
                    sql.push_str(", ");
                }
                let _ = write!(sql, "{} AS \"{}\"", column.sql_name, column.rust_name);
                emitted = true;
            }
        }
        if !emitted {
            if let Some(pk_column) = self
                .columns()
                .iter()
                .find(|column| column.sql_name == self.primary_key())
            {
                let _ = write!(sql, "{} AS \"{}\"", pk_column.sql_name, pk_column.rust_name);
            }
        }
        sql
    }
}

/// Anything a write-path query builder needs on top of
/// [`ReadSource`] — create defaults, update / delete policy slots,
/// audit + retention + versioning state, upsert column list, emitted
/// event topics.
///
/// Implemented by [`ModelDescriptor`](super::ModelDescriptor) only.
/// Views do not implement this trait, so the type system refuses to
/// route a view through `CreateRecord` / `UpdateRecord` /
/// `DeleteRecord` / `UpsertModelInput`.
pub trait WriteSource<M, PK>: ReadSource<M, PK> {
    fn create_allow_policies(&self) -> &'static [ReadPolicy];
    fn create_deny_policies(&self) -> &'static [ReadPolicy];
    fn update_allow_policies(&self) -> &'static [ReadPolicy];
    fn update_deny_policies(&self) -> &'static [ReadPolicy];
    fn delete_allow_policies(&self) -> &'static [ReadPolicy];
    fn delete_deny_policies(&self) -> &'static [ReadPolicy];

    fn create_defaults(&self) -> &'static [CreateDefault];
    fn emitted_events(&self) -> &'static [ModelEventKind];

    /// Optimistic-locking version column (`@version`). `None` for
    /// non-versioned models.
    fn version_column(&self) -> Option<&'static str>;

    /// `true` when the model declared `@@audit`.
    fn audit_enabled(&self) -> bool;

    fn pii_columns(&self) -> &'static [&'static str];
    fn sensitive_columns(&self) -> &'static [&'static str];

    /// Soft-delete retention window. Surfaced here (alongside the
    /// soft-delete column on [`ReadSource`]) so the operator's GC
    /// job can read both pieces from one place.
    fn retention_days(&self) -> Option<u32>;

    /// Columns the upsert primitive is allowed to overwrite on
    /// conflict.
    fn upsert_update_columns(&self) -> &'static [&'static str];
}
