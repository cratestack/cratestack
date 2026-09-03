//! #876: `idempotent_by_default` on the generated
//! `RouteTransportDescriptor`, i.e. `@no_idempotency` becoming live on
//! the REST transport.
//!
//! The RPC counterpart is
//! `transport::op_descriptors::tests_idempotency`. The pair *is* the
//! transport-parity guard: a REST schema emits only `ROUTE_TRANSPORTS`
//! and an RPC schema only `OPS`, so no single test can cover both
//! surfaces, and a fix that lands on one silently no-ops on the other
//! (cratestack#474's original shape).

use super::tests::{
    MODEL_SCHEMA, ORDINARY_PROCEDURE_SCHEMA, parse_first_model, parse_first_procedure,
};
use super::{generate_model_transport_constants, generate_procedure_transport_constants};

/// A bare `procedure` (no `mutation` prefix) is `ProcedureKind::Query` —
/// see `cratestack-parser/src/parse/procedures.rs`. Deliberately not
/// `query procedure`, which is cratestack#870's declarative SQL block and
/// requires an `@@sql(...)` body.
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

/// #876's REST half. The RPC counterpart is
/// `op_descriptors::tests::no_idempotency_procedure_descriptor_carries_idempotent_by_default_true`.
/// The pair *is* the transport-parity guard: a REST schema emits only
/// `ROUTE_TRANSPORTS` and an RPC schema only `OPS`, so no single test can
/// cover both surfaces.
#[test]
fn no_idempotency_procedure_route_carries_idempotent_by_default_true() {
    let procedure = parse_first_procedure(NO_IDEMPOTENCY_SCHEMA);

    let tokens = generate_procedure_transport_constants(&procedure)
        .expect("procedure transport constants should generate")
        .to_string();

    assert!(
        tokens.contains("idempotent_by_default : true"),
        "@no_idempotency procedure's REST route should emit \
         idempotent_by_default: true, got: {tokens}",
    );
}

/// Negative control for the test above: the same `mutation procedure`
/// without the attribute must still participate, or that assertion would
/// pass just as well against codegen that hard-coded `true`.
#[test]
fn ordinary_mutation_route_defaults_idempotent_by_default_false() {
    let procedure = parse_first_procedure(ORDINARY_PROCEDURE_SCHEMA);

    let tokens = generate_procedure_transport_constants(&procedure)
        .expect("procedure transport constants should generate")
        .to_string();

    assert!(
        tokens.contains("idempotent_by_default : false"),
        "a mutation without @no_idempotency should default its REST route to \
         idempotent_by_default: false, got: {tokens}",
    );
}

#[test]
fn query_procedure_route_is_idempotent_by_default_true() {
    let procedure = parse_first_procedure(QUERY_PROCEDURE_SCHEMA);

    let tokens = generate_procedure_transport_constants(&procedure)
        .expect("procedure transport constants should generate")
        .to_string();

    assert!(
        tokens.contains("idempotent_by_default : true"),
        "a query procedure is a read; its REST route must be \
         idempotent_by_default: true, got: {tokens}",
    );
}

/// Two reads (`GET` list + `GET` detail) take no reservation; the three
/// writes do. Asserted as counts because all five consts arrive as one
/// token stream here — and the two counts are asymmetric (2 vs 3), so a
/// read/write swap changes them rather than cancelling out.
#[test]
fn model_crud_routes_split_reads_from_writes() {
    let model = parse_first_model(MODEL_SCHEMA);

    let tokens = generate_model_transport_constants(&model).to_string();

    assert_eq!(
        tokens.matches("idempotent_by_default : true").count(),
        2,
        "exactly the two GET routes (list + detail) should skip reservation, \
         got tokens: {tokens}",
    );
    assert_eq!(
        tokens.matches("idempotent_by_default : false").count(),
        3,
        "the three write routes (POST/PATCH/DELETE) should reserve, \
         got tokens: {tokens}",
    );
}
