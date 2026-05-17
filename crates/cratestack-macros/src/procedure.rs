//! Top-level procedure codegen. Emits two modules per `procedure`:
//! the server-side `pub mod <name>` (policy consts, args struct,
//! `authorize{,_with_db}` + `invoke{,_with_db}`) and the lighter
//! client-side equivalent.

mod authorizer;
mod instrument;
mod types;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TypeDecl};
use quote::quote;

use crate::policy::{
    generate_procedure_policy, parse_procedure_allow_expression, parse_procedure_deny_expression,
};
use crate::shared::{doc_attrs, ident, to_snake_case};

use authorizer::{generate_procedure_model_authorizer, parse_procedure_model_authorizer};
use instrument::{
    authorize_fn_tokens, authorize_with_db_fn_tokens, invoke_fn_tokens, invoke_with_db_fn_tokens,
};
use types::{
    generate_client_procedure_args_struct, generate_procedure_args_struct, procedure_output_tokens,
};

pub(crate) use types::procedure_client_output_item_tokens;

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
    let allow_policies = allow_expressions
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, procedure, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let deny_policies = deny_expressions
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, procedure, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let procedure_name = &procedure.name;
    let args_struct = generate_procedure_args_struct(procedure, types, enum_names);
    let output_type = procedure_output_tokens(&procedure.return_type, types, enum_names);

    let authorize_fn = authorize_fn_tokens();
    let authorize_with_db_fn = authorize_with_db_fn_tokens(&model_authorizers);
    let invoke_fn = invoke_fn_tokens();
    let invoke_with_db_fn = invoke_with_db_fn_tokens();

    Ok(quote! {
        #docs
        pub mod #module_ident {
            pub const NAME: &str = #procedure_name;
            pub const ALLOW_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#allow_policies),*];
            pub const DENY_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#deny_policies),*];

            #args_struct

            pub type Output = #output_type;

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

pub(crate) fn generate_procedure_registry_method(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));

    Ok(quote! {
        fn #method_ident(
            &self,
            db: &super::Cratestack,
            ctx: &::cratestack::CoolContext,
            args: #module_ident::Args,
        ) -> impl ::core::future::Future<Output = Result<#module_ident::Output, ::cratestack::CoolError>> + Send;
    })
}
