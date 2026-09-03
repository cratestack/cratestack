//! cratestack#154 / #876: `rate_limited_by_default` and
//! `idempotent_by_default` on the generated `OpDescriptor`.
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

const NO_IDEMPOTENCY_SCHEMA: &str = r#"
type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
  @no_idempotency
"#;

/// A bare `procedure` (no `mutation` prefix) is `ProcedureKind::Query` —
/// see `cratestack-parser/src/parse/procedures.rs`. Not `query
/// procedure`, which is #870's declarative SQL block and needs a body.
const QUERY_PROCEDURE_SCHEMA: &str = r#"
type Ping {
  nonce String
}

procedure lookupPayment(args: Ping): Ping
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

/// cratestack#743: `@@internal("create")` must drop exactly the
/// `create` `OpDescriptor` — nothing advertises the op as callable
/// (design doc §3, RPC unary row) — while leaving the other four
/// intact.
#[test]
fn internal_create_omits_only_the_create_op_descriptor() {
    let model = parse_first_model(
        r#"
model Widget {
  id Int @id

  @@internal("create")
}
"#,
    );

    let descriptors = generate_model_op_descriptors(&model, false);
    let ids: Vec<String> = descriptors
        .iter()
        .map(|tokens| tokens.to_string())
        .collect();

    assert_eq!(
        descriptors.len(),
        4,
        "expected list/get/update/delete: {ids:?}"
    );
    assert!(
        !ids.iter().any(|op| op.contains("\"model.Widget.create\"")),
        "create op id must not be emitted: {ids:?}"
    );
    assert!(ids.iter().any(|op| op.contains("\"model.Widget.list\"")));
    assert!(ids.iter().any(|op| op.contains("\"model.Widget.get\"")));
    assert!(ids.iter().any(|op| op.contains("\"model.Widget.update\"")));
    assert!(ids.iter().any(|op| op.contains("\"model.Widget.delete\"")));
}

/// cratestack#743 negative control: with no `@@internal` attribute at
/// all, nothing is suppressed — proves the previous test's absence
/// assertion is actually pinned to the attribute, not accidental.
#[test]
fn without_internal_attribute_all_five_op_descriptors_are_emitted() {
    let model = parse_first_model(MODEL_SCHEMA);
    let descriptors = generate_model_op_descriptors(&model, false);
    let ids: Vec<String> = descriptors
        .iter()
        .map(|tokens| tokens.to_string())
        .collect();
    assert!(ids.iter().any(|op| op.contains("\"model.Widget.create\"")));
}

/// #876: the attribute stops being inert. A `mutation procedure` would
/// otherwise emit `idempotent_by_default: false` and take a reservation.
#[test]
fn no_idempotency_procedure_descriptor_carries_idempotent_by_default_true() {
    let procedure = parse_first_procedure(NO_IDEMPOTENCY_SCHEMA);

    let tokens = generate_procedure_op_descriptor(&procedure, false).to_string();

    assert!(
        tokens.contains("idempotent_by_default : true"),
        "@no_idempotency mutation should emit idempotent_by_default: true, got: {tokens}",
    );
}

/// Negative control for the test above: without the attribute the same
/// `mutation procedure` must still participate, or the assertion above
/// would pass against a codegen that hard-coded `true`.
#[test]
fn ordinary_mutation_descriptor_defaults_idempotent_by_default_false() {
    let procedure = parse_first_procedure(ORDINARY_PROCEDURE_SCHEMA);

    let tokens = generate_procedure_op_descriptor(&procedure, false).to_string();

    assert!(
        tokens.contains("idempotent_by_default : false"),
        "a mutation without @no_idempotency should default to \
         idempotent_by_default: false, got: {tokens}",
    );
}

/// The half of the predicate that predates #876: reads were always
/// `true`, and moving the check into `transport::idempotency` must not
/// have changed that.
#[test]
fn query_procedure_descriptor_stays_idempotent_by_default_true() {
    let procedure = parse_first_procedure(QUERY_PROCEDURE_SCHEMA);

    let tokens = generate_procedure_op_descriptor(&procedure, false).to_string();

    assert!(
        tokens.contains("idempotent_by_default : true"),
        "a query procedure is a read and must stay idempotent_by_default: true, got: {tokens}",
    );
}

/// Reads take no reservation, writes do — pinned per verb rather than by
/// a count, because a count would pass if `create` and `list` swapped.
#[test]
fn model_crud_op_descriptors_split_reads_from_writes() {
    let model = parse_first_model(MODEL_SCHEMA);

    let descriptors = generate_model_op_descriptors(&model, false);
    let rendered: Vec<String> = descriptors.iter().map(|t| t.to_string()).collect();

    for (op_id, expected) in [
        ("model.Widget.list", "idempotent_by_default : true"),
        ("model.Widget.get", "idempotent_by_default : true"),
        ("model.Widget.create", "idempotent_by_default : false"),
        ("model.Widget.update", "idempotent_by_default : false"),
        ("model.Widget.delete", "idempotent_by_default : false"),
    ] {
        let found = rendered
            .iter()
            .find(|tokens| tokens.contains(&format!("\"{op_id}\"")))
            .unwrap_or_else(|| panic!("no descriptor for {op_id}: {rendered:?}"));
        assert!(
            found.contains(expected),
            "{op_id} should emit `{expected}`, got: {found}",
        );
    }
}
