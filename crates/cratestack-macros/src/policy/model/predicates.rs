//! Scalar `ReadPredicate` token emitters + the relation-wrapping
//! helper that lifts a scalar predicate up through nested relation
//! quantifiers. Also hosts the small shared lookups (model field,
//! auth field, literal parsing) used across [`term`] and [`comparison`].
//!
//! [`term`]: super::term
//! [`comparison`]: super::comparison

use cratestack_core::{Field, TypeArity, TypeDecl};
use quote::quote;

use crate::policy::auth::{find_auth_field, parse_string_literal};

use super::relation_path::RelationPolicyField;

pub(super) fn generate_scalar_bool_predicate(column: &str) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::ReadPredicate::FieldIsTrue {
            column: #column,
        }
    }
}

pub(super) fn generate_scalar_literal_predicate(
    column: &str,
    literal: proc_macro2::TokenStream,
    negate: bool,
) -> proc_macro2::TokenStream {
    if negate {
        quote! {
            ::cratestack::ReadPredicate::FieldNeLiteral {
                column: #column,
                value: #literal,
            }
        }
    } else {
        quote! {
            ::cratestack::ReadPredicate::FieldEqLiteral {
                column: #column,
                value: #literal,
            }
        }
    }
}

pub(super) fn generate_scalar_auth_predicate(
    column: &str,
    auth_field: &str,
    negate: bool,
) -> proc_macro2::TokenStream {
    if negate {
        quote! {
            ::cratestack::ReadPredicate::FieldNeAuth {
                column: #column,
                auth_field: #auth_field,
            }
        }
    } else {
        quote! {
            ::cratestack::ReadPredicate::FieldEqAuth {
                column: #column,
                auth_field: #auth_field,
            }
        }
    }
}

pub(super) fn wrap_relation_predicate(
    relation_field: &RelationPolicyField<'_>,
    predicate: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let mut predicate = predicate;
    for segment in relation_field.relations.iter().rev() {
        let parent_table = segment.link.parent_table.as_str();
        let parent_column = segment.link.parent_column.as_str();
        let related_table = segment.link.related_table.as_str();
        let related_column = segment.link.related_column.as_str();
        let quantifier = match segment.quantifier {
            "to_one" => quote! { ::cratestack::RelationQuantifier::ToOne },
            "some" => quote! { ::cratestack::RelationQuantifier::Some },
            "every" => quote! { ::cratestack::RelationQuantifier::Every },
            "none" => quote! { ::cratestack::RelationQuantifier::None },
            _ => unreachable!("unsupported policy quantifier"),
        };
        predicate = quote! {
            ::cratestack::ReadPredicate::Relation {
                quantifier: #quantifier,
                parent_table: #parent_table,
                parent_column: #parent_column,
                related_table: #related_table,
                related_column: #related_column,
                expr: &::cratestack::PolicyExpr::Predicate(#predicate),
            }
        };
    }
    predicate
}

pub(super) fn find_model_field<'a>(
    model: &'a cratestack_core::Model,
    field: &str,
) -> Result<&'a Field, String> {
    model
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
        .ok_or_else(|| {
            format!(
                "unknown model field `{field}` in read policy for `{}`",
                model.name
            )
        })
}

pub(super) fn ensure_auth_field(
    auth: Option<&cratestack_core::AuthBlock>,
    types: &[TypeDecl],
    field: &str,
) -> Result<(), String> {
    find_auth_field(auth, types, field).map(|_| ())
}

pub(super) fn validate_auth_field_matches_model_field(
    auth: Option<&cratestack_core::AuthBlock>,
    types: &[TypeDecl],
    auth_field: &str,
    field_decl: &Field,
    field_name: &str,
) -> Result<(), String> {
    let auth_field_decl = find_auth_field(auth, types, auth_field)?;
    if auth_field_decl.ty.name != field_decl.ty.name {
        return Err(format!(
            "auth field `{auth_field}` and model field `{field_name}` must share the same type for policy comparisons"
        ));
    }
    Ok(())
}

pub(super) fn parse_policy_literal(
    rhs: &str,
    field: &Field,
) -> Result<proc_macro2::TokenStream, String> {
    match field.ty.name.as_str() {
        "Boolean" if field.ty.arity == TypeArity::Required => match rhs {
            "true" => Ok(quote! { ::cratestack::PolicyLiteral::Bool(true) }),
            "false" => Ok(quote! { ::cratestack::PolicyLiteral::Bool(false) }),
            _ => Err(format!(
                "expected boolean literal for field `{}`",
                field.name
            )),
        },
        "Int" if field.ty.arity == TypeArity::Required => rhs
            .parse::<i64>()
            .map(|value| quote! { ::cratestack::PolicyLiteral::Int(#value) })
            .map_err(|_| format!("expected integer literal for field `{}`", field.name)),
        "String" if field.ty.arity == TypeArity::Required => {
            let value = parse_string_literal(rhs)
                .ok_or_else(|| format!("expected string literal for field `{}`", field.name))?;
            Ok(quote! { ::cratestack::PolicyLiteral::String(#value) })
        }
        _ => Err(format!(
            "literal read policy support is currently limited to required Boolean, Int, and String fields; `{}` is unsupported",
            field.name
        )),
    }
}
