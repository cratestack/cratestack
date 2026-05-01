use cratestack_core::{Procedure, TypeArity, TypeDecl};
use quote::quote;

use super::ast::{generate_policy_ast_tokens, parse_policy_ast};
use super::auth::{find_auth_field, parse_builtin_policy_call, parse_string_literal};

#[derive(Clone)]
struct ProcedurePolicyField {
    ty: cratestack_core::TypeRef,
}

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
            parse_procedure_policy_term(term, procedure, types, auth)
                .map(|predicate| quote! { ::cratestack::ProcedurePolicyExpr::Predicate(#predicate) })
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

fn parse_procedure_policy_term(
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

fn parse_procedure_comparison(
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

fn resolve_procedure_field(
    procedure: &Procedure,
    types: &[TypeDecl],
    field: &str,
) -> Result<ProcedurePolicyField, String> {
    if let Some((root, rest)) = field.split_once('.') {
        let arg = procedure
            .args
            .iter()
            .find(|candidate| candidate.name == root)
            .ok_or_else(|| {
                format!(
                    "unknown procedure input field `{field}` on `{}`",
                    procedure.name
                )
            })?;
        return resolve_type_field_path(types, &arg.ty.name, rest, &procedure.name, field);
    }

    if let Some(arg) = procedure
        .args
        .iter()
        .find(|candidate| candidate.name == field)
    {
        return Ok(ProcedurePolicyField { ty: arg.ty.clone() });
    }

    if let Some(arg) = procedure
        .args
        .iter()
        .find(|candidate| candidate.name == "args")
        && let Ok(field_decl) =
            resolve_type_field_path(types, &arg.ty.name, field, &procedure.name, field)
    {
        return Ok(field_decl);
    }

    Err(format!(
        "unknown procedure input field `{field}` on `{}`",
        procedure.name
    ))
}

fn resolve_type_field_path(
    types: &[TypeDecl],
    type_name: &str,
    path: &str,
    procedure_name: &str,
    original_field: &str,
) -> Result<ProcedurePolicyField, String> {
    let ty = types.iter().find(|candidate| candidate.name == type_name).ok_or_else(|| {
        format!(
            "procedure `{procedure_name}` references unsupported input type `{type_name}` for policy checks"
        )
    })?;
    let Some((head, tail)) = path.split_once('.') else {
        return ty
            .fields
            .iter()
            .find(|candidate| candidate.name == path)
            .map(|candidate| ProcedurePolicyField {
                ty: candidate.ty.clone(),
            })
            .ok_or_else(|| {
                format!("unknown procedure input field `{original_field}` on `{procedure_name}`")
            });
    };
    let field = ty
        .fields
        .iter()
        .find(|candidate| candidate.name == head)
        .ok_or_else(|| {
            format!("unknown procedure input field `{original_field}` on `{procedure_name}`")
        })?;
    resolve_type_field_path(types, &field.ty.name, tail, procedure_name, original_field)
}

fn validate_procedure_field_type_match(
    left: &ProcedurePolicyField,
    right: &ProcedurePolicyField,
    left_name: &str,
    right_name: &str,
) -> Result<(), String> {
    if left.ty.name != right.ty.name || left.ty.arity != right.ty.arity {
        return Err(format!(
            "procedure fields `{left_name}` and `{right_name}` must share the same type for policy comparisons"
        ));
    }
    Ok(())
}

fn parse_procedure_literal(
    rhs: &str,
    field: Option<&ProcedurePolicyField>,
    field_name: &str,
) -> Result<proc_macro2::TokenStream, String> {
    let (field_type, arity) = match field {
        Some(field) => (field.ty.name.as_str(), field.ty.arity),
        None => ("auth", TypeArity::Required),
    };

    match field_type {
        "Boolean" | "auth" if arity == TypeArity::Required && matches!(rhs, "true" | "false") => {
            let value = rhs == "true";
            Ok(quote! { ::cratestack::ProcedurePolicyLiteral::Bool(#value) })
        }
        "Int" if arity == TypeArity::Required => rhs
            .parse::<i64>()
            .map(|value| quote! { ::cratestack::ProcedurePolicyLiteral::Int(#value) })
            .map_err(|_| format!("expected integer literal for procedure field `{field_name}`")),
        "String" | "auth" if arity == TypeArity::Required => {
            let value = parse_string_literal(rhs).ok_or_else(|| {
                format!("expected string literal for procedure field `{field_name}`")
            })?;
            Ok(quote! { ::cratestack::ProcedurePolicyLiteral::String(#value) })
        }
        _ => Err(format!(
            "procedure policy literal support is currently limited to required Boolean, Int, and String fields; `{field_name}` is unsupported"
        )),
    }
}

fn ensure_auth_field(
    auth: Option<&cratestack_core::AuthBlock>,
    types: &[TypeDecl],
    field: &str,
) -> Result<(), String> {
    find_auth_field(auth, types, field).map(|_| ())
}
