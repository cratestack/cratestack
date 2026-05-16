//! Procedure-side policy expressions: `@allow(...)` / `@deny(...)`
//! attributes on procedure declarations. Parses the AST (boolean
//! expressions of input-field / `auth()` predicates) and emits
//! `cratestack::ProcedurePolicy` const-eval values.
//!
//! Three submodules carve up the work:
//! - [`term`]: single-term recognition (builtins, null checks, lone
//!   boolean fields), with dispatch into [`comparison`] for `==`/`!=`.
//! - [`comparison`]: cross-product of (auth | input | literal) on both
//!   sides of a comparison.
//! - [`resolver`]: input-field resolution + literal parsing, shared
//!   between [`term`] and [`comparison`].

mod comparison;
mod resolver;
mod term;

use cratestack_core::{Procedure, TypeDecl};
use quote::quote;

use super::ast::{generate_policy_ast_tokens, parse_policy_ast};

use term::parse_procedure_policy_term;

pub(crate) fn parse_procedure_allow_expression(raw: &str) -> Option<Result<&str, String>> {
    parse_procedure_policy_expression(raw, "@allow")
}

pub(crate) fn parse_procedure_deny_expression(raw: &str) -> Option<Result<&str, String>> {
    parse_procedure_policy_expression(raw, "@deny")
}

pub(crate) fn generate_procedure_policy(
    expression: &str,
    procedure: &Procedure,
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    let ast = parse_policy_ast(expression)?;
    let expr = generate_policy_ast_tokens(
        &ast,
        &|term| {
            parse_procedure_policy_term(term, procedure, types, auth).map(
                |predicate| quote! { ::cratestack::ProcedurePolicyExpr::Predicate(#predicate) },
            )
        },
        quote! { ::cratestack::ProcedurePolicyExpr::And },
        quote! { ::cratestack::ProcedurePolicyExpr::Or },
    )?;

    Ok(quote! {
        ::cratestack::ProcedurePolicy {
            expr: #expr,
        }
    })
}

fn parse_procedure_policy_expression<'a>(
    raw: &'a str,
    directive: &str,
) -> Option<Result<&'a str, String>> {
    let inner = raw
        .trim()
        .strip_prefix(directive)?
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    Some(Ok(inner))
}
