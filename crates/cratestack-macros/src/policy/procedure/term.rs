//! Single-term parsing for procedure policy expressions. Recognizes
//! builtin functions (`hasRole`/`inTenant`), `auth()` null checks,
//! comparisons (dispatched to [`comparison`]), and lone boolean input
//! fields.

use cratestack_core::{Procedure, TypeArity, TypeDecl};
use quote::quote;

use crate::policy::auth::parse_builtin_policy_call;

use super::comparison::parse_procedure_comparison;
use super::resolver::resolve_procedure_field;

pub(super) fn parse_procedure_policy_term(
    term: &str,
    procedure: &Procedure,
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    if term == "auth() != null" {
        return Ok(quote! { ::cratestack::ProcedurePredicate::AuthNotNull });
    }

    if term == "auth() == null" {
        return Ok(quote! { ::cratestack::ProcedurePredicate::AuthIsNull });
    }

    if let Some(function) = parse_builtin_policy_call(term) {
        return parse_builtin_procedure_policy_term(function?);
    }

    if let Some((lhs, rhs)) = term.split_once("==") {
        return parse_procedure_comparison(lhs.trim(), rhs.trim(), procedure, types, auth, false);
    }

    if let Some((lhs, rhs)) = term.split_once("!=") {
        return parse_procedure_comparison(lhs.trim(), rhs.trim(), procedure, types, auth, true);
    }

    let field_decl = resolve_procedure_field(procedure, types, term)?;
    if field_decl.ty.name != "Boolean" || field_decl.ty.arity != TypeArity::Required {
        return Err(format!(
            "boolean procedure policy check `{term}` is only supported for required Boolean input fields"
        ));
    }

    Ok(quote! {
        ::cratestack::ProcedurePredicate::InputFieldIsTrue {
            field: #term,
        }
    })
}

fn parse_builtin_procedure_policy_term(
    (name, value): (&str, &str),
) -> Result<proc_macro2::TokenStream, String> {
    match name {
        "hasRole" => Ok(quote! {
            ::cratestack::ProcedurePredicate::HasRole {
                role: #value,
            }
        }),
        "inTenant" => Ok(quote! {
            ::cratestack::ProcedurePredicate::InTenant {
                tenant_id: #value,
            }
        }),
        _ => Err(format!("unsupported policy function `{name}`")),
    }
}
