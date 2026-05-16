//! Model-level read policies: `@@allow("action", expr)` /
//! `@@deny("action", expr)` attributes on `model X { ... }` blocks.
//! Parses the expression AST and emits per-action
//! `cratestack::ReadPolicy` values consumed at query time.
//!
//! Four submodules carve up the work:
//! - [`predicates`]: scalar `ReadPredicate` emitters + relation
//!   wrapping + shared field/literal helpers.
//! - [`relation_path`]: `a.b.c` path resolution through relations,
//!   producing the segments [`predicates::wrap_relation_predicate`]
//!   later folds into nested `ReadPredicate::Relation`.
//! - [`term`]: single-term recognition (builtins, `auth()` checks,
//!   bare boolean fields).
//! - [`comparison`]: cross-product of (field | relation | auth |
//!   literal) on either side of `==`/`!=`.

mod comparison;
mod predicates;
mod relation_path;
mod term;

use cratestack_core::{Model, TypeDecl};
use quote::quote;

use super::ast::{generate_policy_ast_tokens, parse_policy_ast};

use term::parse_policy_term;

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
