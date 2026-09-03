//! Adaptor from [`Query`] to [`Procedure`] for the two generators a
//! `query` reuses wholesale: the policy resolver
//! (`crate::policy::generate_procedure_policy`) and the `Args` struct
//! generator (`crate::procedure::generate_procedure_args_struct`).
//!
//! **Why a shim rather than making both generic.** Both generators read
//! exactly three fields — `name` (for error messages), `args` (what
//! predicates and `Args` fields resolve against) and `attributes` — and
//! neither has any model dependency; design §6 verifies this against
//! `policy/procedure/resolver.rs` directly. Introducing a trait or a
//! lifetime-parameterised view over "things with a name and an arg list"
//! would touch every existing call site in `procedure/` to buy nothing a
//! four-field struct literal does not already buy, and every touched call
//! site is a chance to change procedure behaviour while implementing a
//! query feature. The conversion is the smaller blast radius.
//!
//! **The one place this shim's shape is load-bearing.** `kind` is set to
//! [`ProcedureKind::Query`], which the two consumers never read — but if a
//! future generator starts branching on `kind`, a `query` would silently
//! take the read-procedure branch. Anything reading `kind` must therefore
//! take a real `Procedure`, not one of these; that is why this function is
//! `pub(super)` and not exported from `crate::query`.

use cratestack_core::{Procedure, ProcedureKind, Query};

/// A `Procedure`-shaped view of `query`, for the shared generators.
///
/// `return_type` carries the query's declared result type so an `Args`
/// generator that ever consults it sees something coherent; nothing in
/// the current call paths reads it.
pub(super) fn as_procedure(query: &Query) -> Procedure {
    Procedure {
        docs: query.docs.clone(),
        name: query.name.clone(),
        name_span: query.name_span,
        kind: ProcedureKind::Query,
        args: query.args.clone(),
        return_type: query.result_type.clone(),
        attributes: query.attributes.clone(),
        span: query.span,
    }
}
