//! #876: `idempotent_by_default` on the generated `OpDescriptor`, i.e.
//! `@no_idempotency` becoming live on the RPC transport.
//!
//! Split from the sibling `tests.rs` (which keeps cratestack#154's
//! `rate_limited_by_default` assertions and cratestack#743's suppression
//! ones) purely for the 200-line ceiling; the fixture helpers are shared
//! rather than copied, so the two files cannot drift on what a parsed
//! `Procedure` looks like.

use super::tests::{
    MODEL_SCHEMA, ORDINARY_PROCEDURE_SCHEMA, parse_first_model, parse_first_procedure,
};
use super::{generate_model_op_descriptors, generate_procedure_op_descriptor};

/// A bare `procedure` (no `mutation` prefix) is `ProcedureKind::Query` —
/// see `cratestack-parser/src/parse/procedures.rs`. Not `query
/// procedure`, which is #870's declarative SQL block and needs a body.
const QUERY_PROCEDURE_SCHEMA: &str = r#"
type Ping {
  nonce String
}

procedure lookupPayment(args: Ping): Ping
"#;

const NO_IDEMPOTENCY_SCHEMA: &str = r#"
type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
  @no_idempotency
"#;

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
