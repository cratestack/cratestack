use crate::ModelDescriptor;

/// Typed handle for an `.include(...)` call on a query builder. Carries
/// everything the runtime needs to issue the side-load query for a
/// to-one relation: a function pointer that extracts the FK value from
/// a parent row, and a static descriptor of the related model.
///
/// Built by the macro-emitted `<model_module>::<relation_name>()`
/// accessor — see the `.include(...)` builder method on `FindMany`.
///
/// **Scope (v1):** to-one relations only, where the related target
/// column is the related model's primary key. Non-PK references and
/// to-many relations are out of scope for this release; the macro
/// silently omits accessors for non-PK references, and to-many
/// relations stay on the existing list-side query path.
pub struct RelationInclude<M: 'static, Rel: 'static, RelPK: 'static> {
    /// Extracts the FK value from a parent row. `None` ⇒ the parent's
    /// FK column is null, so there's no related row to load. Function
    /// pointers (not closures) by design: keep the type cheap to copy
    /// and ensure call sites can't smuggle in captures that outlive
    /// the descriptor's `'static`.
    pub parent_fk_extract: fn(&M) -> Option<RelPK>,
    /// The related model's descriptor. The runtime uses this to drive
    /// the side-load query (`SELECT projection FROM related WHERE
    /// related.pk IN (...)`) so the related-side read policy still
    /// applies.
    pub related_descriptor: &'static ModelDescriptor<Rel, RelPK>,
}

impl<M, Rel, RelPK> Copy for RelationInclude<M, Rel, RelPK> {}
impl<M, Rel, RelPK> Clone for RelationInclude<M, Rel, RelPK> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, Rel, RelPK> std::fmt::Debug for RelationInclude<M, Rel, RelPK> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelationInclude")
            .field("related_table", &self.related_descriptor.table_name)
            .field("related_primary_key", &self.related_descriptor.primary_key)
            .finish()
    }
}
