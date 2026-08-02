//! Procedure-attribute predicates. Currently just `@stream` — see
//! `crate::procedure::generate_procedure_registry_method` and
//! `crate::axum::procedure` for the two call sites that need to agree on
//! whether a procedure is stream-shaped.

use cratestack_core::Procedure;

/// Procedure carries a bare `@stream` attribute — opts a `T[]`-returning
/// procedure's generated `ProcedureRegistry` trait method into a
/// stream-shaped return instead of the default buffered
/// `Future<Output = Result<Vec<T>, _>>`. `cratestack-parser` rejects
/// `@stream` on a non-list return type before macro codegen ever runs
/// (see `cratestack_parser::validate::stream_attribute`), so callers here
/// may assume list arity whenever this returns `true`.
pub(crate) fn is_stream_procedure(procedure: &Procedure) -> bool {
    procedure
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@stream")
}
