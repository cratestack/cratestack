//! Top-level procedure codegen. Emits two modules per `procedure`:
//! the server-side `pub mod <name>` (policy consts, args struct,
//! `authorize{,_with_db}` + `invoke{,_with_db}`) and the lighter
//! client-side equivalent.

mod authorizer;
mod instrument;
#[cfg(test)]
mod tests;
mod type_tokens;
mod types;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TypeDecl};
use quote::quote;

use crate::policy::{
    PolicySubject, generate_procedure_policy, parse_procedure_allow_expression,
    parse_procedure_deny_expression,
};
use crate::shared::{doc_attrs, ident, is_stream_procedure, to_snake_case};

use authorizer::{generate_procedure_model_authorizer, parse_procedure_model_authorizer};
use instrument::{
    authorize_fn_tokens, authorize_with_db_fn_tokens, authorized_type_tokens, invoke_fn_tokens,
    invoke_with_db_fn_tokens,
};
use types::{generate_client_procedure_args_struct, procedure_stream_item_tokens};

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
    let mut allow_expressions = Vec::new();
    let mut deny_expressions = Vec::new();
    let mut model_authorizers = Vec::new();
    for attribute in &procedure.attributes {
        if let Some(expression) = parse_procedure_allow_expression(&attribute.raw) {
            allow_expressions.push(expression?);
        }
        if let Some(expression) = parse_procedure_deny_expression(&attribute.raw) {
            deny_expressions.push(expression?);
        }
        if let Some(authorizer) = parse_procedure_model_authorizer(&attribute.raw) {
            model_authorizers.push(generate_procedure_model_authorizer(
                authorizer?,
                procedure,
                models,
                types,
            )?);
        }
    }
    let subject = PolicySubject::procedure(procedure);
    let allow_policies = allow_expressions
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, &subject, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let deny_policies = deny_expressions
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
    let invoke_with_db_fn = invoke_with_db_fn_tokens();
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

    Ok(quote! {
        fn #method_ident(
            &self,
            db: &super::Cratestack,
            ctx: &::cratestack::CratestackContext,
            args: #module_ident::Args,
            _authorized: #module_ident::Authorized,
        ) -> impl ::core::future::Future<Output = Result<#module_ident::Output, ::cratestack::CratestackError>> + Send;
    })
}
