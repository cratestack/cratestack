//! Parsing for the model-level `@@internal("action")` attribute
//! (cratestack#743, implementing the accepted design in
//! `docs/design/route-suppression.md`) — an author declaration that a
//! model action must never be reachable from the wire: no REST route,
//! no RPC dispatch arm, no client stub, on any surface. Shares its
//! action vocabulary with `@@allow`/`@@deny`
//! (`cratestack-macros/src/policy/model.rs`'s `parse_rule_action`),
//! but unlike those this attribute takes exactly one action per
//! declaration and carries no policy expression — it is purely a
//! generation-time routing decision (design doc §2.2: "this must
//! never be reachable from the wire, independent of whether some
//! future policy edit would make it satisfiable").
//!
//! [`model_internal_actions`] is the single shared source of truth
//! every surface (REST route assembly, RPC dispatch-arm collection,
//! and every client's per-action stub emission) consults exactly
//! once — see the design doc §3.1 for why routing everything through
//! one function, rather than each surface re-scanning `attribute.raw`
//! independently, is load-bearing rather than merely tidy.

use std::collections::BTreeSet;

use super::model::Model;

/// Action names `@@internal(...)` accepts — identical to `@@allow`'s
/// vocabulary (`list`/`detail`/`read`/`create`/`update`/`delete`/
/// `all`; see `cratestack-macros/src/policy/model.rs`'s
/// `parse_rule_action` and `model/descriptor.rs`'s action groupings)
/// so an author never has to learn a second action vocabulary to
/// suppress what `@@allow` already describes.
pub const INTERNAL_ACTIONS: [&str; 7] = [
    "list", "detail", "read", "create", "update", "delete", "all",
];

/// Parses one `@@internal("action")` attribute's action name and
/// validates it against [`INTERNAL_ACTIONS`]. Returns `Err` naming the
/// model and the bad action for anything else — the compile-error case
/// the design's acceptance criteria requires ("`@@internal` naming an
/// action that is not a valid action verb ⇒ compile error naming the
/// model and the bad action").
pub fn parse_internal_attribute<'a>(model_name: &str, raw: &'a str) -> Result<&'a str, String> {
    let inner = raw
        .trim()
        .strip_prefix("@@internal(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| {
            format!(
                "model `{model_name}` has malformed `@@internal(...)` attribute `{raw}`; \
                 expected `@@internal(\"action\")`"
            )
        })?;
    let action = parse_quoted_action(inner).ok_or_else(|| {
        format!(
            "model `{model_name}` has malformed `@@internal(...)` attribute `{raw}`; expected a \
             single quoted action like `@@internal(\"create\")`"
        )
    })?;
    if !INTERNAL_ACTIONS.contains(&action) {
        return Err(format!(
            "model `{model_name}` declares `@@internal(\"{action}\")`, but `{action}` is not a \
             valid action; expected one of {INTERNAL_ACTIONS:?}"
        ));
    }
    Ok(action)
}

fn parse_quoted_action(inner: &str) -> Option<&str> {
    let inner = inner.trim();
    let mut chars = inner.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &inner[quote.len_utf8()..];
    let end = rest.find(quote)?;
    let action = &rest[..end];
    let remainder = rest[end + quote.len_utf8()..].trim();
    if !remainder.is_empty() {
        return None;
    }
    Some(action)
}

/// Expands one `@@internal` action name to the concrete wire verb(s)
/// it suppresses. `"detail"` maps to the `get` verb (matching
/// `model/descriptor.rs`'s `["detail", "read"]` policy grouping for
/// the single-item read path); `"read"` and `"all"` expand to more
/// than one verb, mirroring the same umbrella groupings `@@allow`
/// already uses for policy actions.
fn expand_action(action: &str) -> &'static [&'static str] {
    match action {
        "list" => &["list"],
        "detail" => &["get"],
        "read" => &["list", "get"],
        "create" => &["create"],
        "update" => &["update"],
        "delete" => &["delete"],
        "all" => &["list", "get", "create", "update", "delete"],
        _ => &[],
    }
}

/// The single shared source of truth every surface consults exactly
/// once: the set of wire verbs (`"list"`, `"get"`, `"create"`,
/// `"update"`, `"delete"`) a model's `@@internal(...)` attributes
/// suppress. Assumes every attribute already parsed successfully via
/// [`parse_internal_attribute`] — per-declaration validation
/// (`cratestack-parser`'s `validate_internal_attribute`) must run
/// first and reject anything else, mirroring
/// `computed_params_type_name`'s same assume-validated contract.
/// Malformed or unrecognized attributes are silently skipped here
/// rather than panicking: a caller reaching this function after a
/// failed parse would already have surfaced the error at schema
/// validation time, and this function must stay infallible so every
/// codegen surface can call it without threading a `Result` through
/// unrelated emission code.
pub fn model_internal_actions(model: &Model) -> BTreeSet<&'static str> {
    model
        .attributes
        .iter()
        .filter(|attribute| attribute.raw.starts_with("@@internal("))
        .filter_map(|attribute| parse_internal_attribute(&model.name, &attribute.raw).ok())
        .flat_map(expand_action)
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SourceSpan;
    use crate::schema::model::Attribute;

    fn span() -> SourceSpan {
        SourceSpan {
            start: 0,
            end: 0,
            line: 1,
        }
    }

    fn model_with_attrs(raws: &[&str]) -> Model {
        Model {
            docs: Vec::new(),
            name: "Widget".to_string(),
            name_span: span(),
            fields: Vec::new(),
            attributes: raws
                .iter()
                .map(|raw| Attribute {
                    raw: raw.to_string(),
                    span: span(),
                })
                .collect(),
            span: span(),
        }
    }

    #[test]
    fn parses_valid_action() {
        assert_eq!(
            parse_internal_attribute("Widget", "@@internal(\"create\")"),
            Ok("create")
        );
    }

    #[test]
    fn parses_single_quoted_action() {
        assert_eq!(
            parse_internal_attribute("Widget", "@@internal('create')"),
            Ok("create")
        );
    }

    #[test]
    fn rejects_invalid_action_naming_model_and_action() {
        let error = parse_internal_attribute("Widget", "@@internal(\"frobnicate\")").unwrap_err();
        assert!(
            error.contains("Widget"),
            "error should name the model: {error}"
        );
        assert!(
            error.contains("frobnicate"),
            "error should name the bad action: {error}"
        );
    }

    #[test]
    fn rejects_malformed_attribute() {
        assert!(parse_internal_attribute("Widget", "@@internal(create)").is_err());
        assert!(parse_internal_attribute("Widget", "@@internal()").is_err());
        assert!(parse_internal_attribute("Widget", "@@internal(\"create\", true)").is_err());
    }

    #[test]
    fn no_internal_attributes_yields_empty_set() {
        let model = model_with_attrs(&[]);
        assert!(model_internal_actions(&model).is_empty());
    }

    #[test]
    fn single_action_expands_to_one_verb() {
        let model = model_with_attrs(&["@@internal(\"create\")"]);
        let actions = model_internal_actions(&model);
        assert_eq!(actions, BTreeSet::from(["create"]));
    }

    #[test]
    fn detail_expands_to_get() {
        let model = model_with_attrs(&["@@internal(\"detail\")"]);
        assert_eq!(model_internal_actions(&model), BTreeSet::from(["get"]));
    }

    #[test]
    fn read_expands_to_list_and_get() {
        let model = model_with_attrs(&["@@internal(\"read\")"]);
        assert_eq!(
            model_internal_actions(&model),
            BTreeSet::from(["list", "get"])
        );
    }

    #[test]
    fn all_expands_to_every_wire_verb() {
        let model = model_with_attrs(&["@@internal(\"all\")"]);
        assert_eq!(
            model_internal_actions(&model),
            BTreeSet::from(["list", "get", "create", "update", "delete"])
        );
    }

    #[test]
    fn multiple_attributes_union() {
        let model = model_with_attrs(&["@@internal(\"create\")", "@@internal(\"update\")"]);
        assert_eq!(
            model_internal_actions(&model),
            BTreeSet::from(["create", "update"])
        );
    }
}
