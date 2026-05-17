//! `lhs == rhs` / `lhs != rhs` predicate builders for model policies.
//! The lhs may be a model field, a relation path, or `auth().x`;
//! the rhs may be an auth field, a model field, or a literal. Each
//! combination dispatches to the appropriate `ReadPredicate` variant.

use cratestack_core::{Model, TypeDecl};
use quote::quote;

use super::predicates::{
    ensure_auth_field, find_model_field, generate_scalar_auth_predicate,
    generate_scalar_literal_predicate, parse_policy_literal,
    validate_auth_field_matches_model_field, wrap_relation_predicate,
};
use super::relation_path::{RelationPolicyField, resolve_relation_policy_field};
use crate::shared::to_snake_case;

pub(super) fn parse_model_comparison(
    field: &str,
    rhs: &str,
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    negate: bool,
) -> Result<proc_macro2::TokenStream, String> {
    if let Some(relation_field) = resolve_relation_policy_field(model, models, field)? {
        return parse_relation_comparison(field, rhs, &relation_field, types, auth, negate);
    }

    if let Some(auth_field) = field.strip_prefix("auth().") {
        return parse_auth_side_model_comparison(
            auth_field.trim(),
            rhs,
            model,
            models,
            types,
            auth,
            negate,
        );
    }

    let field_decl = find_model_field(model, field)?;
    let column = to_snake_case(field);
    if let Some(auth_field) = rhs.strip_prefix("auth().") {
        let auth_field = auth_field.trim();
        ensure_auth_field(auth, types, auth_field)?;
        if negate {
            validate_auth_field_matches_model_field(auth, types, auth_field, field_decl, field)?;
        }
        return Ok(if negate {
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
        });
    }

    let literal = parse_policy_literal(rhs, field_decl)?;
    Ok(if negate {
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
    })
}

fn parse_relation_comparison(
    field: &str,
    rhs: &str,
    relation_field: &RelationPolicyField<'_>,
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    negate: bool,
) -> Result<proc_macro2::TokenStream, String> {
    if let Some(auth_field) = rhs.strip_prefix("auth().") {
        let auth_field = auth_field.trim();
        ensure_auth_field(auth, types, auth_field)?;
        validate_auth_field_matches_model_field(
            auth,
            types,
            auth_field,
            relation_field.target_field,
            field,
        )?;
        return Ok(wrap_relation_predicate(
            relation_field,
            generate_scalar_auth_predicate(
                relation_field.target_column.as_str(),
                auth_field,
                negate,
            ),
        ));
    }

    let literal = parse_policy_literal(rhs, relation_field.target_field)?;
    Ok(wrap_relation_predicate(
        relation_field,
        generate_scalar_literal_predicate(relation_field.target_column.as_str(), literal, negate),
    ))
}

fn parse_auth_side_model_comparison(
    auth_field: &str,
    rhs: &str,
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    negate: bool,
) -> Result<proc_macro2::TokenStream, String> {
    if let Some(relation_field) = resolve_relation_policy_field(model, models, rhs)? {
        ensure_auth_field(auth, types, auth_field)?;
        validate_auth_field_matches_model_field(
            auth,
            types,
            auth_field,
            relation_field.target_field,
            rhs,
        )?;
        return Ok(wrap_relation_predicate(
            &relation_field,
            generate_scalar_auth_predicate(
                relation_field.target_column.as_str(),
                auth_field,
                negate,
            ),
        ));
    }

    if let Ok(field_decl) = find_model_field(model, rhs) {
        let column = to_snake_case(rhs);
        ensure_auth_field(auth, types, auth_field)?;
        validate_auth_field_matches_model_field(auth, types, auth_field, field_decl, rhs)?;
        return Ok(if negate {
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
        });
    }

    let auth_field_decl = crate::policy::auth::find_auth_field(auth, types, auth_field)?;
    let literal = parse_policy_literal(rhs, auth_field_decl)?;
    Ok(if negate {
        quote! {
            ::cratestack::ReadPredicate::AuthFieldNeLiteral {
                auth_field: #auth_field,
                value: #literal,
            }
        }
    } else {
        quote! {
            ::cratestack::ReadPredicate::AuthFieldEqLiteral {
                auth_field: #auth_field,
                value: #literal,
            }
        }
    })
}
