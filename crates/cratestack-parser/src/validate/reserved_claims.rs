//! The principal contract's reserved claim names (ADR 0021 §2,
//! cratestack#1243) in an `auth` block.
//!
//! A schema may declare a reserved name, so a policy can read it through
//! `auth().<name>`, but only with the contract's type: the framework's own
//! readers (`hasRole` over `roles`, `subject()` over `sub`) read a claim of
//! another type as absent, so a schema that declared `roles String` would
//! type-check `auth().roles == "admin"` against a claim the framework never
//! grants on. The list is `cratestack_core::RESERVED_CLAIMS`, one source for
//! the readers and this check. A dotted entry (`tenant.id`, `cnf.jkt`) is
//! checked on the member of the `type` its root field names.

use std::borrow::Cow;

use cratestack_core::{AuthBlock, ClaimType, Field, RESERVED_CLAIMS, Schema, TypeArity, TypeRef};

use crate::diagnostics::{SchemaError, span_error};

#[cfg(test)]
mod tests;

pub(super) fn validate_reserved_claims(
    schema: &Schema,
    auth: &AuthBlock,
) -> Result<(), SchemaError> {
    for field in &auth.fields {
        for reserved in RESERVED_CLAIMS {
            match reserved.path.split_once('.') {
                None if reserved.path == field.name => {
                    check(schema, field, reserved.path, reserved.ty, &auth.name)?;
                }
                Some((root, member)) if root == field.name => {
                    let Some(member) = member_of(schema, &field.ty, member) else {
                        continue;
                    };
                    check(schema, member, reserved.path, reserved.ty, &auth.name)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The member `name` of the `type` declaration `ty` names, if `ty` names
/// one (a list of them is no facet, so it has none).
fn member_of<'a>(schema: &'a Schema, ty: &TypeRef, name: &str) -> Option<&'a Field> {
    if ty.arity == TypeArity::List {
        return None;
    }
    schema
        .types
        .iter()
        .find(|decl| decl.name == ty.name)?
        .fields
        .iter()
        .find(|member| member.name == name)
}

fn check(
    schema: &Schema,
    field: &Field,
    path: &str,
    expected: ClaimType,
    auth_name: &str,
) -> Result<(), SchemaError> {
    let ty = &field.ty;
    let list = ty.arity == TypeArity::List;
    let fits = match expected {
        ClaimType::String => !list && is_string(schema, &ty.name),
        ClaimType::StringList => list && is_string(schema, &ty.name),
        ClaimType::Int => !list && ty.name == "Int",
        ClaimType::Map => {
            !list && (ty.name == "Json" || schema.types.iter().any(|decl| decl.name == ty.name))
        }
    };
    if fits {
        return Ok(());
    }
    let (must, declare) = match expected {
        ClaimType::String => ("a string", "`String` (or a string enum)"),
        ClaimType::StringList => ("a list of strings", "`String[]`"),
        ClaimType::Int => ("an integer", "`Int`"),
        ClaimType::Map => ("a map", "a `type` (or `Json`)"),
    };
    Err(span_error(
        format!(
            "`{path}` on auth block `{auth_name}` is declared `{}`, but `{path}` is a reserved \
             claim of the principal contract and must be {must}; declare it as {declare}, or \
             rename the field",
            render(ty)
        ),
        field.span,
    ))
}

/// A type whose claim is a string on the wire: `String`, `Cuid`, `Uuid`,
/// or a string enum. An ordinal enum travels as an integer, so it is not one.
/// `refuse_ordinal_auth_fields` sees only the `auth` block's own fields, and a
/// dotted claim (`tenant.id`, `cnf.jkt`) is a member of a `type`, so the check
/// is made here too.
fn is_string(schema: &Schema, name: &str) -> bool {
    matches!(name, "String" | "Cuid" | "Uuid")
        || schema
            .enums
            .iter()
            .any(|e| e.name == name && !e.is_ordinal())
}

fn render(ty: &TypeRef) -> Cow<'_, str> {
    match ty.arity {
        TypeArity::Required => Cow::Borrowed(&ty.name),
        TypeArity::Optional => Cow::Owned(format!("{}?", ty.name)),
        TypeArity::List => Cow::Owned(format!("{}[]", ty.name)),
    }
}
