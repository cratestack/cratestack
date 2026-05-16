//! Procedure input field resolution + type compatibility checks +
//! literal parsing used by the policy comparison builder.

use cratestack_core::{Procedure, TypeArity, TypeDecl};
use quote::quote;

use crate::policy::auth::{find_auth_field, parse_string_literal};

#[derive(Clone)]
pub(super) struct ProcedurePolicyField {
    pub(super) ty: cratestack_core::TypeRef,
}

pub(super) fn resolve_procedure_field(
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

pub(super) fn validate_procedure_field_type_match(
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

pub(super) fn parse_procedure_literal(
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

pub(super) fn ensure_auth_field(
    auth: Option<&cratestack_core::AuthBlock>,
    types: &[TypeDecl],
    field: &str,
) -> Result<(), String> {
    find_auth_field(auth, types, field).map(|_| ())
}
