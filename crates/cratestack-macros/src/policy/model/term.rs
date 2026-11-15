//! Single-term recognition for model `@@allow`/`@@deny` expressions.
//! Dispatches the easy cases inline (builtin functions, `auth()`
//! nullity, `auth() == relation`) and defers comparison-style terms
//! to [`super::comparison`].

use cratestack_core::{EnumDecl, Model, TypeArity, TypeDecl};
use quote::quote;

use cratestack_policy::{ModelTerm, read_model_term};

use crate::relation::parse_relation_attribute;
use crate::shared::to_snake_case;

use super::comparison::parse_model_comparison;
use super::in_list::parse_model_in_comparison;
use super::predicates::{
    ensure_auth_field, find_model_field, generate_scalar_bool_predicate, wrap_relation_predicate,
};
use super::relation_path::resolve_relation_policy_field;

pub(super) fn parse_policy_term(
    term: &str,
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    enums: &[EnumDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    _action: &str,
) -> Result<proc_macro2::TokenStream, String> {
    match read_model_term(term) {
        ModelTerm::AuthNotNull => Ok(quote! { ::cratestack::ReadPredicate::AuthNotNull }),
        ModelTerm::AuthIsNull => Ok(quote! { ::cratestack::ReadPredicate::AuthIsNull }),
        // `auth().isSystem()` (issue #486 / ADR 0038 blocker B1): the reader
        // recognises it before the builtin-call shape, which would find
        // `auth`'s `(` and misreport it.
        ModelTerm::AuthIsSystem => Ok(quote! { ::cratestack::ReadPredicate::AuthIsSystem }),
        ModelTerm::Builtin(call) => parse_builtin_model_policy_term(call?),
        ModelTerm::AuthRelation(relation_field) => {
            parse_auth_relation_equality(model, auth, types, relation_field)
        }
        // Set membership is read before comparison, and comparisons split
        // outside string literals, so `label in ["== x"]` is membership.
        ModelTerm::In { lhs, list, negate } => {
            parse_model_in_comparison(lhs, list, model, models, enums, negate)
        }
        ModelTerm::Comparison { lhs, rhs, negate } => {
            parse_model_comparison(lhs, rhs, model, models, types, enums, auth, negate)
        }
        ModelTerm::Bare(term) => parse_bare_model_term(term, model, models),
    }
}

fn parse_bare_model_term(
    term: &str,
    model: &Model,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    if let Some(relation_field) = resolve_relation_policy_field(model, models, term)? {
        if relation_field.target_field.ty.name != "Boolean"
            || relation_field.target_field.ty.arity != TypeArity::Required
        {
            return Err(format!(
                "boolean relation policy check `{term}` is only supported for required Boolean relation fields"
            ));
        }
        return Ok(wrap_relation_predicate(
            &relation_field,
            generate_scalar_bool_predicate(relation_field.target_column.as_str()),
        ));
    }

    let field_decl = find_model_field(model, term)?;
    if field_decl.ty.name != "Boolean" || field_decl.ty.arity != TypeArity::Required {
        return Err(format!(
            "boolean policy check `{term}` is only supported for required Boolean fields"
        ));
    }
    let column = field_decl.sql_column_name();
    Ok(quote! {
        ::cratestack::ReadPredicate::FieldIsTrue {
            column: #column,
        }
    })
}

fn parse_builtin_model_policy_term(
    (name, value): (&str, &str),
) -> Result<proc_macro2::TokenStream, String> {
    match name {
        "hasRole" => Ok(quote! {
            ::cratestack::ReadPredicate::HasRole {
                role: #value,
            }
        }),
        "inTenant" => Ok(quote! {
            ::cratestack::ReadPredicate::InTenant {
                tenant_id: #value,
            }
        }),
        _ => Err(format!("unsupported policy function `{name}`")),
    }
}

fn parse_auth_relation_equality(
    model: &Model,
    auth: Option<&cratestack_core::AuthBlock>,
    types: &[TypeDecl],
    relation_field: &str,
) -> Result<proc_macro2::TokenStream, String> {
    ensure_auth_field(auth, types, "id")?;
    let relation = find_model_field(model, relation_field)?;
    let relation_attribute = parse_relation_attribute(relation).ok_or_else(|| {
        format!(
            "auth relation equality requires `{relation_field}` to be a relation field on `{}`",
            model.name
        )
    })?;

    if relation_attribute.fields.len() != 1 || relation_attribute.references.len() != 1 {
        return Err(format!(
            "auth relation equality only supports single-column relations for `{relation_field}` on `{}`",
            model.name
        ));
    }

    if relation_attribute.references[0] != "id" {
        return Err(format!(
            "auth relation equality currently requires `{relation_field}` on `{}` to reference `id`",
            model.name
        ));
    }

    let local = &relation_attribute.fields[0];
    let column = model
        .sql_column_of(local)
        .unwrap_or_else(|| to_snake_case(local));
    Ok(quote! {
        ::cratestack::ReadPredicate::FieldEqAuth {
            column: #column,
            auth_field: "id",
        }
    })
}
