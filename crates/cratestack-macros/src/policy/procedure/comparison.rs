//! `lhs == rhs` / `lhs != rhs` policy term builders. Handles the
//! cross-product of (auth field | input field | literal) on either
//! side, defers to [`resolver`] for type lookups + literal parsing.

use cratestack_core::{Procedure, TypeDecl};
use quote::quote;

use super::resolver::{
    ensure_auth_field, parse_procedure_literal, resolve_procedure_field,
    validate_procedure_field_type_match,
};

pub(super) fn parse_procedure_comparison(
    lhs: &str,
    rhs: &str,
    procedure: &Procedure,
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    negate: bool,
) -> Result<proc_macro2::TokenStream, String> {
    if let Some(auth_field) = lhs.strip_prefix("auth().") {
        let auth_field = auth_field.trim();
        ensure_auth_field(auth, types, auth_field)?;
        if resolve_procedure_field(procedure, types, rhs).is_ok() {
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

        let literal = parse_procedure_literal(rhs, None, auth_field)?;
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

    let field_decl = resolve_procedure_field(procedure, types, lhs)?;
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

    if let Ok(other_field_decl) = resolve_procedure_field(procedure, types, rhs) {
        validate_procedure_field_type_match(&field_decl, &other_field_decl, lhs, rhs)?;
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

    let literal = parse_procedure_literal(rhs, Some(&field_decl), lhs)?;
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
