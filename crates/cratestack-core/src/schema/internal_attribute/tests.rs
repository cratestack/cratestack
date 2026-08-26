use super::{model_internal_actions, parse_internal_attribute};
use crate::schema::SourceSpan;
use crate::schema::model::{Attribute, Model};
use std::collections::BTreeSet;

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

/// cratestack#743 post-merge review, Finding B: exactly one action per
/// declaration is enforced, not merely documented — two comma-separated
/// quoted actions in one `@@internal(...)` is malformed, the same as any
/// other trailing content after the closing quote. Suppressing two
/// actions means two separate `@@internal("action")` lines
/// (`multiple_attributes_union` below), not one call with two arguments.
#[test]
fn rejects_two_quoted_actions_in_one_declaration() {
    let error =
        parse_internal_attribute("Widget", "@@internal(\"create\", \"update\")").unwrap_err();
    assert!(
        error.contains("Widget"),
        "error should name the model: {error}"
    );
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
