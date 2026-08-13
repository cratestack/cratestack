//! cratestack#154: `rate_limited_by_default` on the generated `OpDescriptor`.
//! Mirrors the fixture-parsing pattern already established in
//! `crate::procedure::tests` (`parse_first_procedure`) — parsing a real
//! `.cstack` snippet through `cratestack_parser::parse_schema` exercises the
//! parser's own `@no_rate_limit` validation (cratestack#154) as well as this
//! module's codegen, rather than hand-building a `Procedure` fixture that
//! could drift from what the parser actually produces.

use super::{generate_model_op_descriptors, generate_procedure_op_descriptor};

fn parse_first_procedure(source: &str) -> cratestack_core::Procedure {
    cratestack_parser::parse_schema(source)
        .expect("fixture schema should parse and validate")
        .procedures
        .remove(0)
}

fn parse_first_model(source: &str) -> cratestack_core::Model {
    cratestack_parser::parse_schema(source)
        .expect("fixture schema should parse and validate")
        .models
        .remove(0)
}

const NO_RATE_LIMIT_SCHEMA: &str = r#"
extension rate_limit {
}

type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
  @no_rate_limit
"#;

const ORDINARY_PROCEDURE_SCHEMA: &str = r#"
type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
"#;

const MODEL_SCHEMA: &str = r#"
model Widget {
  id Int @id
}
"#;

#[test]
fn no_rate_limit_procedure_descriptor_carries_rate_limited_by_default_false() {
    let procedure = parse_first_procedure(NO_RATE_LIMIT_SCHEMA);

    let tokens = generate_procedure_op_descriptor(&procedure, false).to_string();

    assert!(
        tokens.contains("rate_limited_by_default : false"),
        "@no_rate_limit procedure should emit rate_limited_by_default: false, got: {tokens}",
    );
}

#[test]
fn ordinary_procedure_descriptor_defaults_rate_limited_by_default_true() {
    let procedure = parse_first_procedure(ORDINARY_PROCEDURE_SCHEMA);

    let tokens = generate_procedure_op_descriptor(&procedure, false).to_string();

    assert!(
        tokens.contains("rate_limited_by_default : true"),
        "a procedure without @no_rate_limit should default to rate_limited_by_default: true, got: {tokens}",
    );
}

#[test]
fn model_crud_op_descriptors_are_always_rate_limited_by_default() {
    let model = parse_first_model(MODEL_SCHEMA);

    let descriptors = generate_model_op_descriptors(&model, false);

    assert_eq!(
        descriptors.len(),
        5,
        "expected list/get/create/update/delete"
    );
    for tokens in descriptors {
        let rendered = tokens.to_string();
        assert!(
            rendered.contains("rate_limited_by_default : true"),
            "model CRUD ops have no opt-out today and must stay rate_limited_by_default: true, got: {rendered}",
        );
    }
}
