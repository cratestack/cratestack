//! The `ProcedureRegistry` trait method generated for one procedure.

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::{ident, is_stream_procedure, procedure_isolation, to_snake_case};

/// Emits the `ProcedureRegistry` trait method for one procedure. Every
/// `T[]`-returning procedure gets `OpKind::Sequence` at the wire-descriptor
/// level regardless (`crate::transport::op_descriptors`, unchanged by
/// `@stream` — see cratestack#282), but what the trait *implementer*
/// returns differs: a bare `@stream` attribute swaps the default buffered
/// `impl Future<Output = Result<Vec<T>, CratestackError>>` for a
/// `impl Stream<Item = Result<T, CratestackError>>`, so items can be produced
/// incrementally instead of collected up front. Non-`@stream` procedures —
/// which is every procedure today — must keep generating byte-identical
/// tokens to before; see `procedure::tests` for the regression guard.
///
/// Both branches reference the item/output type via the procedure's own
/// `#module_ident::{Output,Item}` alias (see [`generate_procedure_module`])
/// rather than recomputing type tokens here: this trait method is spliced
/// directly under `pub mod procedures` (see
/// `include/server.rs`'s `ProcedureRegistry` trait), one nesting level
/// shallower than the per-procedure module, so a raw `super::super::...`
/// path computed for that deeper context would resolve one level too far
/// up from here. The same reasoning covers the trailing `#module_ident
/// ::Authorized` parameter (cratestack#512): it's the witness type
/// [`instrument::authorized_type_tokens`] splices into this same
/// `#module_ident` module, constructible only by that module's own
/// `authorize_with_db`/`invoke_with_db` — which is what makes
/// `registry.<method>(&db, &ctx, args)` (three arguments, the shape that
/// used to skip every `@allow`) fail to compile instead of silently
/// bypassing policy. An implementor never constructs one; they only
/// receive it (typically as `_authorized`) and, if calling another
/// procedure isn't involved, ignore it.
///
/// **Migration (cratestack#512, breaking):** every existing
/// `ProcedureRegistry` implementor gains this parameter on every method —
/// add `_authorized: <procedure>::Authorized` (any name; it is not read)
/// as the new last parameter. Mechanical, no behavior to reason about: the
/// value has no API surface beyond existing.
pub(crate) fn generate_procedure_registry_method(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));

    if is_stream_procedure(procedure) {
        return Ok(quote! {
            fn #method_ident(
                &self,
                db: &super::Cratestack,
                ctx: &::cratestack::CratestackContext,
                args: #module_ident::Args,
                _authorized: #module_ident::Authorized,
            ) -> impl ::cratestack::futures::Stream<Item = Result<#module_ident::Item, ::cratestack::CratestackError>> + Send;
        });
    }

    // `@isolation`: the handle bound to the procedure's transaction, which
    // has no `pool()` (docs/design/procedure-isolation.md §3).
    // The implementor sees this doc on the trait method it writes: the body
    // is re-run on a serialization failure, so what it does outside `db`
    // happens once per attempt (§5).
    let (db_type, retry_doc) = match procedure_isolation(procedure) {
        Some(level) => {
            let doc = format!(
                " Runs inside one `{}` transaction; on a serialization failure or \
                 deadlock (`40001`/`40P01`) the whole body runs again, up to the retry \
                 budget. Everything done through `db` is rolled back with a failed \
                 attempt; anything else (HTTP calls, e-mail, state in `self`) is \
                 repeated, so make it idempotent or move it behind `@@emit`. The \
                 `ComputedFieldResolver` methods that compose this procedure's output \
                 run inside the same attempt, after the body and before `COMMIT`, and \
                 re-run with it, so they must be re-runnable too. See \
                 docs/design/procedure-isolation.md.",
                level.as_sql(),
            );
            (
                quote! { super::IsolatedCratestack },
                quote! { #[doc = #doc] },
            )
        }
        None => (quote! { super::Cratestack }, quote! {}),
    };
    Ok(quote! {
        #retry_doc
        fn #method_ident(
            &self,
            db: &#db_type,
            ctx: &::cratestack::CratestackContext,
            args: #module_ident::Args,
            _authorized: #module_ident::Authorized,
        ) -> impl ::core::future::Future<Output = Result<#module_ident::Output, ::cratestack::CratestackError>> + Send;
    })
}
