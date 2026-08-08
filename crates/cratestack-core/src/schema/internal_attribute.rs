//! SPIKE (`spike/b1-internal-actions`): `@@internal("update", ...)`
//! — a model-level marker declaring that an action is reachable from
//! server code (procedures, workers) but generates **no REST route**.
//!
//! This is orthogonal to policy. The action's `@@allow` / `@@deny`
//! rules are compiled and evaluated exactly as before; the only thing
//! suppressed is route emission in
//! `cratestack_macros::axum::model::routes`. That splits the two
//! concerns that are currently welded together: today, adding a write
//! policy so server code can use the ORM necessarily also opens a
//! public REST CRUD route for that action.
//!
//! Mirrors [`crate::events::parse_emit_attribute`]'s shape so the
//! parser's validation pass can reuse the same error style.

use std::collections::BTreeSet;

use super::model::Model;

/// Actions `@@internal` accepts. Deliberately the same vocabulary the
/// policy lowering already uses (see
/// `cratestack_macros::model::descriptor`), so a schema author does
/// not have to learn a second action spelling.
pub const INTERNAL_ACTIONS: &[&str] = &[
    "list", "detail", "read", "create", "update", "delete", "all",
];

/// Parse one `@@internal(...)` attribute into its action list.
///
/// Accepts `@@internal("update")`, `@@internal("update", "delete")`
/// and `@@internal("update, delete")` — the last spelling matches how
/// `@@allow` already accepts a comma-joined action string.
pub fn parse_internal_attribute(raw: &str) -> Result<Vec<String>, String> {
    let inner = raw
        .trim()
        .strip_prefix("@@internal(")
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| "`@@internal` must be written `@@internal(\"<action>\", ...)`".to_owned())?
        .trim();

    if inner.is_empty() {
        return Err(
            "`@@internal` requires at least one action, e.g. `@@internal(\"update\")`".to_owned(),
        );
    }

    let mut actions = Vec::new();
    for literal in split_outside_quotes(inner) {
        let literal = literal.trim();
        if literal.is_empty() {
            continue;
        }
        let contents = strip_string_literal(literal).ok_or_else(|| {
            format!("`@@internal` actions must be string literals (got `{literal}`)")
        })?;
        // A single literal may itself list several actions
        // (`@@internal("update, delete")`), matching how `@@allow`
        // already accepts a comma-joined action string.
        for action in contents.split(',') {
            let action = action.trim();
            if action.is_empty() {
                continue;
            }
            if !INTERNAL_ACTIONS.contains(&action) {
                return Err(format!(
                    "`@@internal` action `{action}` is not one of {}",
                    INTERNAL_ACTIONS.join(", ")
                ));
            }
            if !actions.iter().any(|seen: &String| seen == action) {
                actions.push(action.to_owned());
            }
        }
    }

    if actions.is_empty() {
        return Err(
            "`@@internal` requires at least one action, e.g. `@@internal(\"update\")`".to_owned(),
        );
    }

    Ok(actions)
}

/// Every route-suppressed action on a model, expanded so that `read`
/// covers `list` + `detail` and `all` covers everything.
///
/// Malformed attributes are ignored here rather than reported —
/// `cratestack-parser`'s validation pass is the single place that
/// turns them into a schema error, and it runs first.
pub fn model_internal_actions(model: &Model) -> BTreeSet<String> {
    let mut actions = BTreeSet::new();
    for attribute in &model.attributes {
        let Ok(parsed) = parse_internal_attribute(&attribute.raw) else {
            continue;
        };
        for action in parsed {
            match action.as_str() {
                "read" => {
                    actions.insert("list".to_owned());
                    actions.insert("detail".to_owned());
                }
                "all" => {
                    for concrete in ["list", "detail", "create", "update", "delete"] {
                        actions.insert(concrete.to_owned());
                    }
                }
                other => {
                    actions.insert(other.to_owned());
                }
            }
        }
    }
    actions
}

/// Split on commas that sit *outside* a string literal, so
/// `"update", "delete"` yields two literals while `"update, delete"`
/// yields one. Mirrors `split_policy_arguments` in
/// `cratestack_macros::axum::policy_attr`.
fn split_outside_quotes(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

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
            (None, ',') => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    parts.push(current);
    parts
}

fn strip_string_literal(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_and_multiple_actions() {
        assert_eq!(
            parse_internal_attribute(r#"@@internal("update")"#).expect("should parse"),
            vec!["update".to_owned()]
        );
        assert_eq!(
            parse_internal_attribute(r#"@@internal("update", "delete")"#).expect("should parse"),
            vec!["update".to_owned(), "delete".to_owned()]
        );
        assert_eq!(
            parse_internal_attribute(r#"@@internal("update, delete")"#).expect("should parse"),
            vec!["update".to_owned(), "delete".to_owned()]
        );
    }

    #[test]
    fn rejects_unknown_action_and_empty_list() {
        assert!(parse_internal_attribute(r#"@@internal("upsert")"#).is_err());
        assert!(parse_internal_attribute("@@internal()").is_err());
        assert!(parse_internal_attribute("@@internal").is_err());
        assert!(parse_internal_attribute(r#"@@internal(update)"#).is_err());
    }

    #[test]
    fn non_internal_attributes_are_not_claimed() {
        assert!(parse_internal_attribute("@@paged").is_err());
        assert!(parse_internal_attribute(r#"@@allow("read", true)"#).is_err());
    }
}
