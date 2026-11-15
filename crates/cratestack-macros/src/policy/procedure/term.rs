//! Single-term parsing for procedure policy expressions. Recognizes
//! builtin functions (`hasRole`/`inTenant`), `auth()` null checks,
//! comparisons (dispatched to [`comparison`]), and lone boolean input
//! fields.

use cratestack_core::{TypeArity, TypeDecl};
use quote::quote;

use cratestack_policy::{ProcedureTerm, read_procedure_term};

use super::comparison::parse_procedure_comparison;
use super::resolver::resolve_procedure_field;
use super::subject::PolicySubject;

pub(super) fn parse_procedure_policy_term(
    term: &str,
    subject: &PolicySubject<'_>,
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    // The shape of the term is `cratestack_policy::read_procedure_term`'s, the
    // reader `cratestack contract ops` classifies from. `auth().isSystem()`
    // is recognised before the builtin-call shape (cratestack#867's review),
    // and a bare `true`/`false` is the whole clause, not an input field.
    match read_procedure_term(term) {
        ProcedureTerm::AuthNotNull => {
            return Ok(quote! { ::cratestack::ProcedurePredicate::AuthNotNull });
        }
        ProcedureTerm::AuthIsNull => {
            return Ok(quote! { ::cratestack::ProcedurePredicate::AuthIsNull });
        }
        ProcedureTerm::True => {
            return Ok(quote! { ::cratestack::ProcedurePredicate::Literal(true) });
        }
        ProcedureTerm::False => {
            return Ok(quote! { ::cratestack::ProcedurePredicate::Literal(false) });
        }
        ProcedureTerm::AuthIsSystem => {
            return Ok(quote! { ::cratestack::ProcedurePredicate::AuthIsSystem });
        }
        ProcedureTerm::Builtin(call) => return parse_builtin_procedure_policy_term(call?),
        ProcedureTerm::Comparison { lhs, rhs, negate } => {
            return parse_procedure_comparison(lhs, rhs, subject, types, auth, negate);
        }
        ProcedureTerm::Bare(_) => {}
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
