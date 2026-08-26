//! cratestack#743: RPC dispatch-arm suppression. A suppressed verb
//! must not produce a `quote!` arm at all — the design's whole point
//! is that `rpc_dispatch_inner`'s pre-existing unknown-op-id catch-all
//! (`rpc_module.rs`) does the rest, with no new runtime branch here.

use super::generate_model_rpc_dispatch_arms;

fn parse_first_model(source: &str) -> cratestack_core::Model {
    cratestack_parser::parse_schema(source)
        .expect("fixture schema should parse and validate")
        .models
        .remove(0)
}

const MODEL_SCHEMA: &str = r#"
model Widget {
  id Int @id
}
"#;

#[test]
fn without_internal_attribute_all_five_arms_are_emitted() {
    let model = parse_first_model(MODEL_SCHEMA);
    let arms = generate_model_rpc_dispatch_arms(&model);
    assert_eq!(arms.len(), 5, "expected list/get/create/update/delete");
    let rendered: Vec<String> = arms.iter().map(|arm| arm.to_string()).collect();
    for op in [
        "model.Widget.list",
        "model.Widget.get",
        "model.Widget.create",
        "model.Widget.update",
        "model.Widget.delete",
    ] {
        assert!(
            rendered
                .iter()
                .any(|arm| arm.contains(&format!("\"{op}\""))),
            "missing arm for {op}: {rendered:?}"
        );
    }
}

/// The negative-control pin: suppressing `create` removes exactly that
/// arm, leaving the other four in place.
#[test]
fn internal_create_omits_only_the_create_arm() {
    let model = parse_first_model(
        r#"
model Widget {
  id Int @id

  @@internal("create")
}
"#,
    );
    let arms = generate_model_rpc_dispatch_arms(&model);
    let rendered: Vec<String> = arms.iter().map(|arm| arm.to_string()).collect();

    assert_eq!(
        arms.len(),
        4,
        "expected list/get/update/delete: {rendered:?}"
    );
    assert!(
        !rendered
            .iter()
            .any(|arm| arm.contains("\"model.Widget.create\"")),
        "create arm must be omitted: {rendered:?}"
    );
    for op in [
        "model.Widget.list",
        "model.Widget.get",
        "model.Widget.update",
        "model.Widget.delete",
    ] {
        assert!(
            rendered
                .iter()
                .any(|arm| arm.contains(&format!("\"{op}\""))),
            "missing arm for {op}: {rendered:?}"
        );
    }
}

#[test]
fn internal_all_omits_every_arm() {
    let model = parse_first_model(
        r#"
model Widget {
  id Int @id

  @@internal("all")
}
"#,
    );
    let arms = generate_model_rpc_dispatch_arms(&model);
    assert!(arms.is_empty(), "expected no arms at all: {arms:?}");
}

/// Models with no primary key already can't dispatch get/update/delete
/// (each verb gets a `rpc_dispatch_error` fallback arm instead of a
/// real one) — suppression must filter this defensive fallback path
/// too, not just the happy path. `cratestack-parser` rejects a
/// pk-less model outright (`validate::models`: "missing an @id
/// field"), so this constructs the IR directly rather than through
/// `parse_schema`, exactly like `generate_model_rpc_dispatch_arms`'s
/// own doc comment describes this branch as defensive/unreachable in
/// practice today.
#[test]
fn internal_filters_the_no_primary_key_fallback_too() {
    use cratestack_core::{Attribute, Field, Model, SourceSpan, TypeArity, TypeRef};

    fn span() -> SourceSpan {
        SourceSpan {
            start: 0,
            end: 0,
            line: 1,
        }
    }

    let model = Model {
        docs: Vec::new(),
        name: "Widget".to_string(),
        name_span: span(),
        fields: vec![Field {
            docs: Vec::new(),
            name: "name".to_string(),
            name_span: span(),
            ty: TypeRef {
                name: "String".to_string(),
                name_span: span(),
                arity: TypeArity::Required,
                generic_args: Vec::new(),
                int_args: Vec::new(),
            },
            attributes: Vec::new(),
            span: span(),
        }],
        attributes: vec![Attribute {
            raw: "@@internal(\"create\")".to_string(),
            span: span(),
        }],
        span: span(),
    };

    let arms = generate_model_rpc_dispatch_arms(&model);
    let rendered: Vec<String> = arms.iter().map(|arm| arm.to_string()).collect();
    assert_eq!(
        arms.len(),
        4,
        "expected list/get/update/delete: {rendered:?}"
    );
    assert!(
        !rendered
            .iter()
            .any(|arm| arm.contains("\"model.Widget.create\"")),
        "create arm must be omitted even on the no-pk fallback path: {rendered:?}"
    );
}
