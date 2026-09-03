//! `lhs == rhs` / `lhs != rhs` policy term builders. Handles the
//! cross-product of (auth field | input field | literal) on either
//! side, defers to [`resolver`] for type lookups + literal parsing.

use cratestack_core::TypeDecl;
use quote::quote;

use super::resolver::{
    ensure_auth_field, parse_procedure_literal, resolve_procedure_field,
    validate_procedure_field_type_match,
};
use super::subject::PolicySubject;

pub(super) fn parse_procedure_comparison(
    lhs: &str,
    rhs: &str,
    subject: &PolicySubject<'_>,
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    negate: bool,
) -> Result<proc_macro2::TokenStream, String> {
    if let Some(auth_field) = lhs.strip_prefix("auth().") {
        let auth_field = auth_field.trim();
        ensure_auth_field(auth, types, auth_field)?;
        if resolve_procedure_field(subject, types, rhs).is_ok() {
            return Ok(if negate {
                quote! {
                    ::cratestack::ProcedurePredicate::InputFieldNeAuth {
                        field: #rhs,
                        auth_field: #auth_field,
                    }
                }
            } else {
                quote! {
                    ::cratestack::ProcedurePredicate::InputFieldEqAuth {
                        field: #rhs,
                        auth_field: #auth_field,
                    }
                }
            });
        }

        // The RHS did not resolve as an input field. If it *looks*
        // like one — a bare identifier, not a quoted string, number or
        // boolean — then the author almost certainly mistyped an argument
        // name, and reporting "expected string literal for field
        // `subjectId`" (which is what happened before cratestack#867's
        // review) points at the wrong half of the comparison entirely.
        if looks_like_an_identifier(rhs) {
            return Err(subject.unknown_field(rhs));
        }
        let literal = parse_procedure_literal(rhs, None, auth_field, subject)?;
        return Ok(if negate {
            quote! {
                ::cratestack::ProcedurePredicate::AuthFieldNeLiteral {
                    auth_field: #auth_field,
                    value: #literal,
                }
            }
        } else {
            quote! {
                ::cratestack::ProcedurePredicate::AuthFieldEqLiteral {
                    auth_field: #auth_field,
                    value: #literal,
                }
            }
        });
    }

    let field_decl = resolve_procedure_field(subject, types, lhs)?;
    if let Some(auth_field) = rhs.strip_prefix("auth().") {
        let auth_field = auth_field.trim();
        ensure_auth_field(auth, types, auth_field)?;
        return Ok(if negate {
            quote! {
                ::cratestack::ProcedurePredicate::InputFieldNeAuth {
                    field: #lhs,
                    auth_field: #auth_field,
                }
            }
        } else {
            quote! {
                ::cratestack::ProcedurePredicate::InputFieldEqAuth {
                    field: #lhs,
                    auth_field: #auth_field,
                }
            }
        });
    }

    if let Ok(other_field_decl) = resolve_procedure_field(subject, types, rhs) {
        validate_procedure_field_type_match(&field_decl, &other_field_decl, lhs, rhs, subject)?;
        return Ok(if negate {
            quote! {
                ::cratestack::ProcedurePredicate::InputFieldNeInput {
                    field: #lhs,
                    other_field: #rhs,
                }
            }
        } else {
            quote! {
                ::cratestack::ProcedurePredicate::InputFieldEqInput {
                    field: #lhs,
                    other_field: #rhs,
                }
            }
        });
    }

    if looks_like_an_identifier(rhs) {
        return Err(subject.unknown_field(rhs));
    }
    let literal = parse_procedure_literal(rhs, Some(&field_decl), lhs, subject)?;
    Ok(if negate {
        quote! {
            ::cratestack::ProcedurePredicate::InputFieldNeLiteral {
                field: #lhs,
                value: #literal,
            }
        }
    } else {
        quote! {
            ::cratestack::ProcedurePredicate::InputFieldEqLiteral {
                field: #lhs,
                value: #literal,
            }
        }
    })
}

/// Whether `rhs` reads as a bare identifier rather than a literal.
///
/// Used only to pick which *diagnostic* to emit, never to change what
/// compiles: a value reaching here has already failed to resolve as an
/// input field, so both branches are errors. `true`/`false` are excluded
/// because they are genuine boolean literals on this path, and anything
/// starting with a digit or a quote cannot be an argument name.
fn looks_like_an_identifier(rhs: &str) -> bool {
    if matches!(rhs, "true" | "false") {
        return false;
    }
    let mut chars = rhs.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}
