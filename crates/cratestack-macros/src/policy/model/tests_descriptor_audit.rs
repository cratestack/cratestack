//! The model and view descriptors run the policy re-check
//! (GHSA-69g4-xvcm-vm2j): a rule their exact reader would skip is a
//! compile error at the call site, not only in `audit_model_policies`.
//! The IR is edited by hand, as a parser bug would hand it over. Removing
//! either `audit_model_policies` call left every other test green.

use crate::model::generate_model_descriptor;
use crate::view::generate_view_descriptor;

const SCHEMA: &str = r#"
auth SessionUser {
  id Int
  role String
}

model Note {
  id Int @id
  body String
  @@allow("all", auth() != null)
  @@deny("read", auth().role == "banned")
}

view NoteView from Note {
  id Int @id @from(Note.id)
  @@server_sql("SELECT id FROM notes")
  @@allow("read", auth() != null)
  @@deny("read", auth().role == "banned")
}
"#;

const SKIPPED: &str = "GHSA-69g4-xvcm-vm2j";

fn model_result(raw: &str) -> Result<String, String> {
    let mut schema = cratestack_parser::parse_schema(SCHEMA).expect("fixture parses");
    schema.models[0].attributes[1].raw = raw.to_owned();
    let model = &schema.models[0];
    generate_model_descriptor(
        model,
        &schema.models,
        &schema.types,
        &[],
        schema.auth.as_ref(),
    )
    .map(|tokens| tokens.to_string())
}

fn view_result(raw: &str) -> Result<String, String> {
    let mut schema = cratestack_parser::parse_schema(SCHEMA).expect("fixture parses");
    schema.views[0].attributes[2].raw = raw.to_owned();
    let view = &schema.views[0];
    generate_view_descriptor(
        view,
        &schema.models,
        &schema.types,
        &[],
        schema.auth.as_ref(),
    )
    .map(|tokens| tokens.to_string())
}

#[test]
fn the_descriptors_generate_the_canonical_rules() {
    let canonical = "@@deny(\"read\", auth().role == \"banned\")";
    model_result(canonical).unwrap_or_else(|error| panic!("model generates: {error}"));
    view_result(canonical).unwrap_or_else(|error| panic!("view generates: {error}"));
}

#[test]
fn the_descriptors_refuse_a_rule_their_reader_skips() {
    for raw in [
        "@@deny (\"read\", auth().role == \"banned\")",
        "@@de\u{200B}ny(\"read\", auth().role == \"banned\")",
        "@@deny(\"raed\", auth().role == \"banned\")",
    ] {
        let message = model_result(raw).expect_err(raw);
        assert!(message.contains(SKIPPED), "{message}");
    }
    // A view builds only its `read` slot: a `list` deny would apply nowhere.
    for raw in [
        "@@deny(\"list\", auth().role == \"banned\")",
        "@@Deny(\"read\", auth().role == \"banned\")",
    ] {
        let message = view_result(raw).expect_err(raw);
        assert!(message.contains(SKIPPED), "{message}");
    }
}

/// Maintainer decision 4: the parser now accepts a view's single-quoted
/// `@@allow('read', …)`, and the view generator applies it exactly as
/// the double-quoted rule (the generated descriptor is identical).
#[test]
fn a_single_quoted_view_allow_generates_the_same_rule() {
    let generate = |allow: &str| {
        const VIEW_ALLOW: &str = "@@allow(\"read\", auth() != null)";
        assert!(
            SCHEMA.contains(VIEW_ALLOW),
            "the fixture carries the view allow"
        );
        let source = SCHEMA.replace(VIEW_ALLOW, allow);
        let schema = cratestack_parser::parse_schema(&source).expect("the view parses");
        let view = &schema.views[0];
        let auth = schema.auth.as_ref();
        generate_view_descriptor(view, &schema.models, &schema.types, &[], auth)
            .expect("the view generates")
            .to_string()
    };
    let double = generate("@@allow(\"read\", auth() != null)");
    assert_eq!(generate("@@allow('read', auth() != null)"), double);
    assert_ne!(
        generate("@@allow('read', auth().role == \"admin\")"),
        double
    );
}
