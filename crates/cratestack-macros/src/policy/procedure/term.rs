//! Single-term parsing for procedure policy expressions. Recognizes
//! builtin functions (`hasRole`/`inTenant`), `auth()` null checks,
//! comparisons (dispatched to [`comparison`]), and lone boolean input
//! fields.

use cratestack_core::{TypeArity, TypeDecl};
use quote::quote;

use crate::policy::auth::{is_auth_is_system_term, parse_builtin_policy_call};

use super::comparison::parse_procedure_comparison;
use super::resolver::resolve_procedure_field;
use super::subject::PolicySubject;

pub(super) fn parse_procedure_policy_term(
    term: &str,
    subject: &PolicySubject<'_>,
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    if term == "auth() != null" {
        return Ok(quote! { ::cratestack::ProcedurePredicate::AuthNotNull });
    }

    if term == "auth() == null" {
        return Ok(quote! { ::cratestack::ProcedurePredicate::AuthIsNull });
    }

    // A bare boolean literal as the WHOLE clause, e.g. `@allow(true)`.
    // Checked before field resolution below, which would otherwise treat
    // `true`/`false` as an unresolved input-field name and fail with
    // "unknown procedure input field `true`" — a comparison RHS like
    // `auth() == true` already supports bool literals via
    // `parse_procedure_literal`, but that path only runs once `==`/`!=`
    // has been split out, which a bare `true`/`false` term never contains.
    if term == "true" {
        return Ok(quote! { ::cratestack::ProcedurePredicate::Literal(true) });
    }

    if term == "false" {
        return Ok(quote! { ::cratestack::ProcedurePredicate::Literal(false) });
    }

    // `auth().isSystem()` (cratestack#486, wired to this dialect by
    // cratestack#867's review). Must be recognised *before*
    // `parse_builtin_policy_call`, which finds the first `(` in the term
    // — for `auth().isSystem()` that is `auth`'s, so it would otherwise
    // report "policy function `auth` requires a string literal argument"
    // instead of lowering the term. Exactly the mis-diagnosis the review
    // hit. Mirrors `policy/model/term.rs`'s ordering for the same reason.
    if is_auth_is_system_term(term) {
        return Ok(quote! { ::cratestack::ProcedurePredicate::AuthIsSystem });
    }

    if let Some(function) = parse_builtin_policy_call(term) {
        return parse_builtin_procedure_policy_term(function?);
    }

    if let Some((lhs, rhs)) = term.split_once("==") {
        return parse_procedure_comparison(lhs.trim(), rhs.trim(), subject, types, auth, false);
    }

    if let Some((lhs, rhs)) = term.split_once("!=") {
        return parse_procedure_comparison(lhs.trim(), rhs.trim(), subject, types, auth, true);
    }

    let field_decl = resolve_procedure_field(subject, types, term)?;
    if field_decl.ty.name != "Boolean" || field_decl.ty.arity != TypeArity::Required {
        return Err(format!(
            "boolean {} policy check `{term}` is only supported for required Boolean input fields",
            subject.construct,
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
