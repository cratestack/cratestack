//! `@@allow(...)` attribute parsing — used to decide whether the
//! generated create handler needs an explicit `is_authenticated()`
//! preflight (when every create allow rule reduces to `auth() != null`).

use cratestack_core::Model;

pub(super) fn create_requires_authenticated_context(model: &Model) -> bool {
    let mut saw_create_allow = false;
    for attribute in &model.attributes {
        let Some((actions, expression)) = parse_model_allow_attribute(&attribute.raw) else {
            continue;
        };
        if !actions
            .iter()
            .any(|action| matches!(action.as_str(), "create" | "all"))
        {
            continue;
        }

        saw_create_allow = true;
        if normalize_policy_expression(&expression) != "auth()!=null" {
            return false;
        }
    }

    saw_create_allow
}

fn parse_model_allow_attribute(raw: &str) -> Option<(Vec<String>, String)> {
    let inner = raw
        .strip_prefix("@@allow(")?
        .strip_suffix(')')?
        .trim()
        .to_owned();
    let mut parts = split_policy_arguments(&inner);
    if parts.len() != 2 {
        return None;
    }
    let expression = parts.pop()?.trim().to_owned();
    let actions = trim_policy_string_literal(&parts.pop()?)?
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .filter(|action| !action.is_empty())
        .collect::<Vec<_>>();

    Some((actions, expression))
}

fn split_policy_arguments(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut depth = 0usize;

    for character in value.chars() {
        match (quote, character) {
            (Some(active), candidate) if active == candidate => {
                quote = None;
                current.push(character);
            }
            (Some(_), _) => current.push(character),
            (None, '\'' | '"') => {
                quote = Some(character);
                current.push(character);
            }
            (None, '(') => {
                depth += 1;
                current.push(character);
            }
            (None, ')') => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            (None, ',') if depth == 0 => {
                parts.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(character),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }

    parts
}

fn trim_policy_string_literal(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|candidate| candidate.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|candidate| candidate.strip_suffix('\''))
        })
}

fn normalize_policy_expression(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
