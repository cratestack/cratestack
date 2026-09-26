//! What a relation subquery may see of its *related* table.
//!
//! A relation filter (`EXISTS (SELECT 1 FROM related WHERE ...)`) or a
//! relation sort (`(SELECT related.col FROM related WHERE ... LIMIT 1)`)
//! reads the related table directly. Unless that subquery applies the
//! related model's own read policy and soft-delete filter, a caller can
//! test and order by values of rows they cannot read — rows `?include=`
//! correctly reports as `null` / absent (GHSA-p55v-6xv5-93p3).
//!
//! Every public constructor of a relation hop, relation filter or
//! relation sort therefore takes a [`RelatedReadScope`]; there is no
//! default. Generated code passes the related model's
//! [`ModelDescriptor::related_read_scope`]. Hand-written code that
//! genuinely wants the raw table passes [`RelatedReadScope::Unscoped`],
//! which is the named escape hatch and never the fallback.

use cratestack_policy::ReadPolicy;

use crate::descriptor::ModelDescriptor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedReadScope {
    /// Apply the related model's list-slot read policy (`@@allow`/`@@deny`
    /// on `read`/`list`) and its `@@soft_delete` filter inside the
    /// subquery — exactly what `find_many` on that model applies, which
    /// is also what `?include=` goes through. A related row the caller
    /// cannot read then behaves as if it did not exist: a to-one filter
    /// (including `ne` and `isNull`) does not match it, `none`/`every`
    /// over hidden-only children are vacuously true, and a relation sort
    /// key reads as `NULL`.
    ///
    /// An empty `allow` slice renders `FALSE` (default deny), the same as
    /// a direct read of a model with no read rule.
    Policy {
        allow: &'static [ReadPolicy],
        deny: &'static [ReadPolicy],
        /// `Some("deleted_at")` when the related model is `@@soft_delete`.
        soft_delete_column: Option<&'static str>,
    },
    /// Escape hatch: read the related table raw — no policy, no
    /// soft-delete filter. For trusted server code that deliberately wants
    /// every related row regardless of the caller. The embedded (rusqlite)
    /// backend ignores the scope either way. Generated client code (which
    /// never renders SQL) also uses it. Never use it for a filter or sort
    /// whose values a caller controls.
    Unscoped,
}

impl<M, PK> ModelDescriptor<M, PK> {
    /// The scope a relation subquery into this model must apply: the same
    /// list-slot policies and soft-delete filter `find_many` applies.
    pub const fn related_read_scope(&self) -> RelatedReadScope {
        RelatedReadScope::Policy {
            allow: self.read_allow_policies,
            deny: self.read_deny_policies,
            soft_delete_column: self.soft_delete_column,
        }
    }
}
