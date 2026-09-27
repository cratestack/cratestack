//! `@@allow` / `@@deny` on a `model` or `view`, in the one spelling the
//! generator reads (GHSA-69g4-xvcm-vm2j).
//!
//! `cratestack-macros/src/policy/model.rs` takes a rule only when the line
//! is exactly `@@allow(` / `@@deny(` … `)` with a first argument naming an
//! action it generates for, and silently skips anything else. So
//! `@@deny ("read", …)`, `@@Deny(…)`, `@@deny(…);`, `@@deyn(…)` and
//! `@@deny("raed", …)` all passed `cratestack check` and generated a model
//! with no deny rule. Any attribute whose name reads as `allow` or `deny`
//! — in any case, with space after the sigils, or a typo away — must now
//! be exactly one rule the generator will apply.

use cratestack_core::Attribute;
use cratestack_core::schema::attribute_text::{group_end, loose_attribute_names};

use super::misspelled_attributes::optimal_string_alignment;
use crate::diagnostics::{SchemaError, span_error};

/// The actions a model rule may name: `all` and every slot
/// `cratestack-macros/src/model/descriptor.rs` generates.
pub(super) const MODEL_ACTIONS: &[&str] = &[
    "all", "read", "list", "detail", "create", "update", "delete",
];
/// A view generates the `read` slot only (`view/descriptor.rs`), so a
/// deny rule naming `list`, `detail` or a write action was never applied.
/// Its `@@allow` is limited to `read` by `super::views`' own rule, which
/// keeps its message.
pub(super) const VIEW_DENY_ACTIONS: &[&str] = &["all", "read"];

/// Checks one block-level attribute of `owner` (``model `Doc` ``); an
/// `@@allow` may name `allow_actions`, a `@@deny` `deny_actions`.
pub(super) fn validate_policy_attribute(
    owner: &str,
    attribute: &Attribute,
    allow_actions: &[&str],
    deny_actions: &[&str],
) -> Result<(), SchemaError> {
    let raw = attribute.raw.as_str();
    let Some(loose) = loose_attribute_names(raw).into_iter().next() else {
        return Ok(());
    };
    let Some(policy) = ["allow", "deny"]
        .into_iter()
        .find(|name| loose == *name || optimal_string_alignment(&loose, name) <= 1)
    else {
        return Ok(());
    };
    let name = format!("@@{policy}");
    let actions = if policy == "allow" {
        allow_actions
    } else {
        deny_actions
    };
    let refuse = |why: String| {
        Err(span_error(
            format!(
                "{owner} writes `{raw}`: {why}. The generator reads a rule only when it is \
                 written `{name}(\"action\", expression)` and silently skips anything else, so \
                 this rule would not be applied; it is refused"
            ),
            attribute.span,
        ))
    };
    let Some(rest) = raw.strip_prefix(&name) else {
        return refuse(format!("this is not spelled `{name}`"));
    };
    if !rest.starts_with('(') {
        return refuse(format!("`{name}` must be followed directly by `(`"));
    }
    let Some(end) = group_end(raw, name.len()) else {
        return refuse("its argument list is never closed on this line".to_owned());
    };
    if !raw[end..].trim().is_empty() {
        return refuse(format!(
            "`{}` follows the closing `)`; remove it or make it a `//` comment",
            raw[end..].trim()
        ));
    }
    let inner = raw[name.len() + 1..end - 1].trim();
    let Some(quote) = inner.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
        return refuse("its first argument must be a quoted action".to_owned());
    };
    let Some((action, expression)) = inner[1..].split_once(quote) else {
        return refuse("its action string is never closed".to_owned());
    };
    if !actions.contains(&action) {
        return refuse(format!(
            "`{action}` is not an action here (expected one of {})",
            actions.join(", ")
        ));
    }
    let expression = expression.trim_start().strip_prefix(',').map(str::trim);
    if expression.is_none_or(str::is_empty) {
        return refuse("it has no expression after the action".to_owned());
    }
    Ok(())
}
