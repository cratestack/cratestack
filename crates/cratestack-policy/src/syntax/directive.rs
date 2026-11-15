//! Pulling the expression out of an `@allow(...)` / `@deny(...)` procedure
//! attribute and an `@@allow("action", ...)` / `@@deny(...)` model one.

/// The inner expression of a procedure/query `directive` (`@allow` or
/// `@deny`), or `None` when `raw` is some other attribute.
pub fn procedure_rule_expression<'a>(
    raw: &'a str,
    directive: &str,
) -> Option<Result<&'a str, String>> {
    let inner = raw
        .trim()
        .strip_prefix(directive)?
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    Some(Ok(inner))
}

/// The expression of a model `directive` (`@@allow` or `@@deny`) rule, when
/// its action is one of `actions` or `all`; `None` when `raw` is another
/// attribute or names another action.
pub fn model_rule_expression<'a>(
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
