//! Single-term recognition for model `@@allow`/`@@deny` expressions.
//! Dispatches the easy cases inline (builtin functions, `auth()`
//! nullity, `auth() == relation`) and defers comparison-style terms
//! to [`super::comparison`].

use cratestack_core::{Model, TypeArity, TypeDecl};
use quote::quote;

use crate::policy::auth::parse_builtin_policy_call;
use crate::relation::parse_relation_attribute;
use crate::shared::to_snake_case;

use super::comparison::parse_model_comparison;
use super::predicates::{
    ensure_auth_field, find_model_field, generate_scalar_bool_predicate, wrap_relation_predicate,
};
use super::relation_path::resolve_relation_policy_field;

pub(super) fn parse_policy_term(
    term: &str,
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    _action: &str,
) -> Result<proc_macro2::TokenStream, String> {
    if term == "auth() != null" {
        return Ok(quote! { ::cratestack::ReadPredicate::AuthNotNull });
    }

    if term == "auth() == null" {
        return Ok(quote! { ::cratestack::ReadPredicate::AuthIsNull });
    }

    if let Some(function) = parse_builtin_policy_call(term) {
        return parse_builtin_model_policy_term(function?);
    }

    if let Some(relation_field) = term.strip_prefix("auth() ==") {
        return parse_auth_relation_equality(model, auth, types, relation_field.trim());
    }

    if let Some(relation_field) = term.strip_suffix("== auth()") {
        return parse_auth_relation_equality(model, auth, types, relation_field.trim());
    }

    if let Some((field, rhs)) = term.split_once("==") {
        return parse_model_comparison(field.trim(), rhs.trim(), model, models, types, auth, false);
    }

    if let Some((field, rhs)) = term.split_once("!=") {
        return parse_model_comparison(field.trim(), rhs.trim(), model, models, types, auth, true);
    }

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
    let column = to_snake_case(term);
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

    let column = to_snake_case(&relation_attribute.fields[0]);
    Ok(quote! {
        ::cratestack::ReadPredicate::FieldEqAuth {
            column: #column,
            auth_field: "id",
        }
    })
}
