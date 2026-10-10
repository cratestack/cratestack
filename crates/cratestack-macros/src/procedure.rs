//! Top-level procedure codegen. Emits two modules per `procedure`:
//! the server-side `pub mod <name>` (policy consts, args struct,
//! `authorize{,_with_db}` + `invoke{,_with_db}`) and the lighter
//! client-side equivalent.

mod authorizer;
mod client_types;
mod instrument;
mod policies;
mod registry_method;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_isolation;
#[cfg(test)]
mod tests_policy_audit;
#[cfg(test)]
mod tests_policy_audit_selectors;
#[cfg(test)]
mod tests_validation;
mod type_tokens;
mod types;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TypeDecl};
use quote::quote;

use crate::policy::{PolicySubject, generate_procedure_policy};
use crate::shared::{doc_attrs, ident, is_stream_procedure, to_snake_case};
use crate::validators::Validating;

use client_types::generate_client_procedure_args_struct;
use instrument::{
    authorize_fn_tokens, authorize_with_db_fn_tokens, authorized_type_tokens, invoke_fn_tokens,
    isolation_and_invoke_with_db_tokens,
};
use policies::collect_procedure_policies;
pub(crate) use registry_method::generate_procedure_registry_method;
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
    validating: &Validating,
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
    let args_struct =
        generate_procedure_args_struct(procedure, types, enum_names, "procedure", validating);
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
