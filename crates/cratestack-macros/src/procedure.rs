//! Top-level procedure codegen. Emits two modules per `procedure`:
//! the server-side `pub mod <name>` (policy consts, args struct,
//! `authorize{,_with_db}` + `invoke{,_with_db}`) and the lighter
//! client-side equivalent.

mod authorizer;
mod client_types;
mod instrument;
mod policies;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_isolation;
#[cfg(test)]
mod tests_policy_audit;
#[cfg(test)]
mod tests_policy_audit_selectors;
mod type_tokens;
mod types;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TypeDecl};
use quote::quote;

use crate::policy::{PolicySubject, generate_procedure_policy};
use crate::shared::{doc_attrs, ident, is_stream_procedure, procedure_isolation, to_snake_case};

use client_types::generate_client_procedure_args_struct;
use instrument::{
    authorize_fn_tokens, authorize_with_db_fn_tokens, authorized_type_tokens, invoke_fn_tokens,
    isolation_and_invoke_with_db_tokens,
};
use policies::collect_procedure_policies;
use types::procedure_stream_item_tokens;

pub(crate) use types::procedure_client_output_item_tokens;
/// Re-exported for `crate::query` (cratestack#867): a `query`'s `Args`
/// struct and result-type tokens are a procedure's, resolved against the
/// same `type`/`enum` declarations at the same module depth. Sharing the
/// generator is what makes the policy resolver — which reads `Args`
/// through the `ProcedureArgs` impl emitted here — work for a `query`
/// with no new machinery (design §6).
pub(crate) use types::{generate_procedure_args_struct, procedure_output_tokens};

pub(crate) fn generate_procedure_module(
    procedure: &Procedure,
    models: &[Model],
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&procedure.name));
    let docs = doc_attrs(&procedure.docs);
    let policies = collect_procedure_policies(procedure, models, types)?;
    let model_authorizers = policies.authorizers;
    let subject = PolicySubject::procedure(procedure);
    let allow_policies = policies
        .allow
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, &subject, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let deny_policies = policies
        .deny
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, &subject, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let procedure_name = &procedure.name;
    let args_struct = generate_procedure_args_struct(procedure, types, enum_names, "procedure");
    let output_type = procedure_output_tokens(&procedure.return_type, types, enum_names);
    // `@stream` procedures additionally get a `pub type Item = T;` alias
    // (the list's element type, not `Vec<T>`) alongside `Output` — the
    // registry trait method (`generate_procedure_registry_method`)
    // references it as `#module_ident::Item` for the same reason the
    // non-stream trait method references `#module_ident::Output` instead
    // of recomputing the type tokens itself: this module, not the trait
    // (which lives one level up, directly under `pub mod procedures`),
    // is at the right nesting depth for `types`/model paths to resolve.
    let item_type_alias = if is_stream_procedure(procedure) {
        let item_type = procedure_stream_item_tokens(&procedure.return_type, types, enum_names);
        quote! { pub type Item = #item_type; }
    } else {
        quote! {}
    };

    let authorize_fn = authorize_fn_tokens();
    let authorize_with_db_fn = authorize_with_db_fn_tokens(&model_authorizers);
    let invoke_fn = invoke_fn_tokens();
    let (isolation_const, invoke_with_db_fn) = isolation_and_invoke_with_db_tokens(procedure);
    // cratestack#512: the witness type `authorize_with_db`/`invoke_with_db`
    // are the only source of — see `instrument::authorized_type_tokens`'s
    // doc comment for why its private field, not any convention, is what
    // makes the `ProcedureRegistry` trait method below uncallable without
    // going through one of them first.
    let authorized_type = authorized_type_tokens();

    Ok(quote! {
        #docs
        pub mod #module_ident {
            pub const NAME: &str = #procedure_name;
            pub const ALLOW_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#allow_policies),*];
            pub const DENY_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#deny_policies),*];
            #isolation_const

            #args_struct

            pub type Output = #output_type;
            #item_type_alias
            #authorized_type

            #authorize_fn
            #authorize_with_db_fn
            #invoke_fn
            #invoke_with_db_fn
        }
    })
}

pub(crate) fn generate_client_procedure_module(
    procedure: &Procedure,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&procedure.name));
    let docs = doc_attrs(&procedure.docs);
    let procedure_name = &procedure.name;
    let args_struct = generate_client_procedure_args_struct(procedure, types, enum_names);
    let output_type = procedure_output_tokens(&procedure.return_type, types, enum_names);

    Ok(quote! {
        #docs
        pub mod #module_ident {
            pub const NAME: &str = #procedure_name;

            #args_struct

            pub type Output = #output_type;
        }
    })
}

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
