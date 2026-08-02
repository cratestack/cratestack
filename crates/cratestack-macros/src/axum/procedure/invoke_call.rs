//! Bridges the `ProcedureRegistry` trait method's return shape — a
//! `Future` for ordinary procedures, a `Stream` for `@stream`-marked ones
//! (see `crate::procedure::generate_procedure_registry_method`) — back to
//! the single `Result<Output, CoolError>` shape the rest of the dispatch
//! pipeline (`invoke_with_db`, the transport result encoder) already
//! expects.
//!
//! `@stream` procedures are buffered into a `Vec` right here, via
//! `TryStreamExt::try_collect`, which is *exactly* today's wire behavior
//! for every `T[]`-returning procedure (`OpKind::Sequence` is still
//! arity-driven and still encoded as one buffered payload — see
//! `crate::transport::op_descriptors`, deliberately unchanged by this
//! ticket). Swapping this buffering for genuinely incremental encoding is
//! cratestack-axum's concern, tracked separately (cratestack#283,
//! `docs/design/rpc-transport.md` §3.3) — this module is the seam that
//! ticket replaces; it does not touch the wire format itself.

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::is_stream_procedure;

/// Token stream for the call inside `invoke_with_db`'s closure that
/// actually invokes the registry method: `registry.<method>(&db, &ctx,
/// args)`, either `.await`ed directly (ordinary procedures) or streamed
/// and buffered into a `Vec` (`@stream` procedures).
pub(super) fn procedure_invoke_call_tokens(
    procedure: &Procedure,
    method_ident: &syn::Ident,
) -> proc_macro2::TokenStream {
    if is_stream_procedure(procedure) {
        quote! {
            {
                use ::cratestack::futures::TryStreamExt as _;
                registry.#method_ident(&db, &call_ctx, call_args).try_collect::<Vec<_>>().await
            }
        }
    } else {
        quote! {
            registry.#method_ident(&db, &call_ctx, call_args).await
        }
    }
}
