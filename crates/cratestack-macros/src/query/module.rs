//! The per-query `pub mod <query_snake>` assembly (cratestack#867).
//!
//! Shape deliberately parallels a procedure's generated module — `NAME`,
//! `ALLOW_POLICIES`, `DENY_POLICIES`, `Args`, `Output` — so that a reader
//! who knows one knows the other. What is *absent* is the point: no
//! `authorize`/`invoke` pair, no `Authorized` witness, and no registry
//! trait method, because a `query` has exactly one entry point
//! ([`super::entry`]) and therefore nothing for a witness to guard.

use std::collections::BTreeSet;

use cratestack_core::{Query, TypeArity, TypeDecl};
use quote::quote;

use crate::policy::{
    generate_procedure_policy, parse_procedure_allow_expression, parse_procedure_deny_expression,
};
use crate::procedure::{generate_procedure_args_struct, procedure_output_tokens};
use crate::shared::{doc_attrs, ident, to_snake_case};

use super::entry::generate_query_entry;
use super::shim::as_procedure;

pub(crate) fn generate_query_module(
    query: &Query,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&query.name));
    let docs = doc_attrs(&query.docs);
    let query_name = &query.name;

    // The policy resolver is the procedure one, reached through the shim —
    // see `super::shim`'s doc comment for why this is a conversion rather
    // than a generalization.
    let procedure = as_procedure(query);
    let mut allow_expressions = Vec::new();
    let mut deny_expressions = Vec::new();
    for attribute in &query.attributes {
        if let Some(expression) = parse_procedure_allow_expression(&attribute.raw) {
            allow_expressions.push(expression?);
        }
        if let Some(expression) = parse_procedure_deny_expression(&attribute.raw) {
            deny_expressions.push(expression?);
        }
    }
    let allow_policies = allow_expressions
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, &procedure, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let deny_policies = deny_expressions
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, &procedure, types, auth))
        .collect::<Result<Vec<_>, _>>()?;

    let args_struct = generate_procedure_args_struct(&procedure, types, enum_names);
    let output_type = procedure_output_tokens(&query.result_type, types, enum_names);
    // `query_as::<_, T>` decodes one row at a time, so it always wants the
    // element type — `Vec<T>` for a `T[]` query is what `fetch_all`
    // produces, not what the row decoder is parameterized by.
    let element_type = {
        let mut element = query.result_type.clone();
        element.arity = TypeArity::Required;
        procedure_output_tokens(&element, types, enum_names)
    };
    let entry = generate_query_entry(query, &element_type);

    Ok(quote! {
        #docs
        pub mod #module_ident {
            pub const NAME: &str = #query_name;
            pub const ALLOW_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#allow_policies),*];
            pub const DENY_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#deny_policies),*];

            #args_struct

            pub type Output = #output_type;

            #entry
        }
    })
}
