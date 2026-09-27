//! Procedure-attribute predicates: `@stream` — see
//! `crate::procedure::generate_procedure_registry_method` and
//! `crate::axum::procedure` for the two call sites that need to agree on
//! whether a procedure is stream-shaped — and `@isolation`, which every
//! dispatch site (REST/RPC handler, MCP arm, `invoke_with_db`, the
//! registry method) has to agree on too.

use cratestack_core::{Procedure, TransactionIsolation};
use quote::quote;

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

/// The level a procedure's `@isolation("...")` attribute declares, if any.
/// `cratestack-parser` has already validated the attribute (at most one, a
/// quoted level [`TransactionIsolation::parse`] accepts, not on `@stream`),
/// so this only reads it. See docs/design/procedure-isolation.md.
pub(crate) fn procedure_isolation(procedure: &Procedure) -> Option<TransactionIsolation> {
    procedure.attributes.iter().find_map(|attribute| {
        let level = attribute
            .raw
            .strip_prefix("@isolation(")?
            .strip_suffix(')')?
            .trim()
            .strip_prefix('"')?
            .strip_suffix('"')?;
        TransactionIsolation::parse(level).ok()
    })
}

/// `::cratestack::TransactionIsolation::<Variant>` for generated code.
pub(crate) fn isolation_tokens(isolation: TransactionIsolation) -> proc_macro2::TokenStream {
    match isolation {
        TransactionIsolation::ReadCommitted => {
            quote! { ::cratestack::TransactionIsolation::ReadCommitted }
        }
        TransactionIsolation::RepeatableRead => {
            quote! { ::cratestack::TransactionIsolation::RepeatableRead }
        }
        TransactionIsolation::Serializable => {
            quote! { ::cratestack::TransactionIsolation::Serializable }
        }
    }
}
