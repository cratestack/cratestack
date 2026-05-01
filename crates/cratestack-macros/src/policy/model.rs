use cratestack_core::{Field, Model, TypeArity, TypeDecl};
use quote::quote;

use super::ast::{generate_policy_ast_tokens, parse_policy_ast};
use super::auth::{find_auth_field, parse_builtin_policy_call, parse_string_literal};
use crate::relation::{RelationLink, parse_relation_attribute, relation_link};
use crate::shared::{find_model, is_relation_field, model_name_set, to_snake_case};

pub(crate) fn generate_policies_for_action(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    action: &str,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    generate_policies_for_actions(model, models, types, auth, &[action])
}

pub(crate) fn generate_policies_for_actions(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    actions: &[&str],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    generate_policy_rules_for_actions(model, models, types, auth, actions, "@@allow")
}

pub(crate) fn generate_denies_for_action(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    action: &str,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    generate_denies_for_actions(model, models, types, auth, &[action])
}

pub(crate) fn generate_denies_for_actions(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    actions: &[&str],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    generate_policy_rules_for_actions(model, models, types, auth, actions, "@@deny")
}

#[derive(Clone)]
struct RelationPolicySegment {
    link: RelationLink,
    quantifier: &'static str,
}

struct RelationPolicyField<'a> {
    relations: Vec<RelationPolicySegment>,
    target_field: &'a Field,
    target_column: String,
}

fn generate_policy_rules_for_actions(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    actions: &[&str],
    directive: &str,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let mut policies = Vec::new();
    for attribute in &model.attributes {
        if let Some(expression) = parse_policy_expression(&attribute.raw, directive, actions) {
            let primary_action = actions.first().copied().unwrap_or("read");
            policies.push(generate_read_policy(
                expression?,
                model,
                models,
                types,
                auth,
                primary_action,
            )?);
        }
    }
    Ok(policies)
}

fn parse_policy_expression<'a>(
    raw: &'a str,
    directive: &str,
    actions: &[&str],
) -> Option<Result<&'a str, String>> {
    let inner = raw
        .trim()
        .strip_prefix(directive)?
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    let primary_action = actions.first().copied().unwrap_or("read");
    let Some((rule_action, rest)) = parse_rule_action(inner) else {
        return Some(Err(format!(
            "invalid {primary_action} policy attribute: {raw}"
        )));
    };
    if rule_action != "all" && !actions.contains(&rule_action) {
        return None;
    }
    let expression = match rest.strip_prefix(',') {
        Some(expression) => expression.trim(),
        None => {
            return Some(Err(format!(
                "invalid {primary_action} policy attribute: {raw}"
            )));
        }
    };
    Some(Ok(expression))
}

fn parse_rule_action(inner: &str) -> Option<(&str, &str)> {
    let mut chars = inner.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &inner[quote.len_utf8()..];
    let end = rest.find(quote)?;
    let action = &rest[..end];
    let remainder = rest[end + quote.len_utf8()..].trim_start();
    Some((action, remainder))
}

fn generate_read_policy(
    expression: &str,
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    action: &str,
) -> Result<proc_macro2::TokenStream, String> {
    let ast = parse_policy_ast(expression)?;
    let expr = generate_policy_ast_tokens(
        &ast,
        &|term| {
            parse_policy_term(term, model, models, types, auth, action)
                .map(|predicate| quote! { ::cratestack::PolicyExpr::Predicate(#predicate) })
        },
        quote! { ::cratestack::PolicyExpr::And },
        quote! { ::cratestack::PolicyExpr::Or },
    )?;

    Ok(quote! {
        ::cratestack::ReadPolicy {
            expr: #expr,
        }
    })
}

fn parse_policy_term(
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

fn parse_model_comparison(
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

    let auth_field_decl = find_auth_field(auth, types, auth_field)?;
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

fn validate_auth_field_matches_model_field(
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

fn resolve_relation_policy_field<'a>(
    model: &'a Model,
    models: &'a [Model],
    path: &str,
) -> Result<Option<RelationPolicyField<'a>>, String> {
    if path.starts_with("auth().") || !path.contains('.') {
        return Ok(None);
    }

    let model_names = model_name_set(models);
    let mut current_model = model;
    let mut relations = Vec::new();
    let parts = path.split('.').collect::<Vec<_>>();
    let mut index = 0usize;
    while let Some(part) = parts.get(index).copied() {
        let field = find_model_field(current_model, part)?;
        if !is_relation_field(&model_names, field) {
            if index + 1 != parts.len() {
                return Err(format!(
                    "relation policy path `{path}` cannot traverse through scalar field `{part}`"
                ));
            }
            return Ok(Some(RelationPolicyField {
                relations,
                target_field: field,
                target_column: to_snake_case(&field.name),
            }));
        }

        if index + 1 == parts.len() {
            return Err(format!(
                "relation policy path `{path}` must end on a scalar field"
            ));
        }

        let link = relation_link(current_model, field, models)?;
        let (quantifier, step) = if field.ty.arity == TypeArity::List {
            let quantifier = match parts.get(index + 1).copied() {
                Some("some") => "some",
                Some("every") => "every",
                Some("none") => "none",
                Some(segment) => {
                    return Err(format!(
                        "relation policy path `{path}` must use `some`, `every`, or `none` after to-many relation `{part}`; found `{segment}`"
                    ));
                }
                None => {
                    return Err(format!(
                        "relation policy path `{path}` must use `some`, `every`, or `none` after to-many relation `{part}`"
                    ));
                }
            };
            (quantifier, 2usize)
        } else {
            if matches!(
                parts.get(index + 1).copied(),
                Some("some" | "every" | "none")
            ) {
                return Err(format!(
                    "relation policy path `{path}` cannot use a collection quantifier after to-one relation `{part}`"
                ));
            }
            ("to_one", 1usize)
        };

        relations.push(RelationPolicySegment { link, quantifier });
        current_model = find_model(models, &field.ty.name).ok_or_else(|| {
            format!(
                "relation policy path `{path}` references unknown target model `{}`",
                field.ty.name
            )
        })?;

        index += step;
        if quantifier != "to_one" && index >= parts.len() {
            return Err(format!(
                "relation policy path `{path}` must continue after `{part}.{quantifier}`"
            ));
        }
    }

    Ok(None)
}

fn generate_scalar_bool_predicate(column: &str) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::ReadPredicate::FieldIsTrue {
            column: #column,
        }
    }
}

fn generate_scalar_literal_predicate(
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

fn generate_scalar_auth_predicate(
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

fn wrap_relation_predicate(
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

fn find_model_field<'a>(model: &'a Model, field: &str) -> Result<&'a Field, String> {
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

fn ensure_auth_field(
    auth: Option<&cratestack_core::AuthBlock>,
    types: &[TypeDecl],
    field: &str,
) -> Result<(), String> {
    find_auth_field(auth, types, field).map(|_| ())
}

fn parse_policy_literal(rhs: &str, field: &Field) -> Result<proc_macro2::TokenStream, String> {
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
