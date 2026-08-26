//! cratestack#474: `rate_limited_by_default` on the generated
//! `RouteTransportDescriptor`. Mirrors
//! `transport::op_descriptors::tests` (the RPC counterpart) so REST and
//! RPC schemas are pinned to the same `@no_rate_limit` semantics — a fix
//! that only covers one transport silently no-ops for the other.

use super::{
    generate_model_transport_constants, generate_model_transport_entries,
    generate_procedure_transport_constants,
};

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

const MODEL_WITH_INTERNAL_CREATE_SCHEMA: &str = r#"
model Widget {
  id Int @id

  @@internal("create")
}
"#;

#[test]
fn no_rate_limit_procedure_route_carries_rate_limited_by_default_false() {
    let procedure = parse_first_procedure(NO_RATE_LIMIT_SCHEMA);

    let tokens = generate_procedure_transport_constants(&procedure)
        .expect("procedure transport constants should generate")
        .to_string();

    assert!(
        tokens.contains("rate_limited_by_default : false"),
        "@no_rate_limit procedure's REST route should emit rate_limited_by_default: false, got: {tokens}",
    );
}

#[test]
fn ordinary_procedure_route_defaults_rate_limited_by_default_true() {
    let procedure = parse_first_procedure(ORDINARY_PROCEDURE_SCHEMA);

    let tokens = generate_procedure_transport_constants(&procedure)
        .expect("procedure transport constants should generate")
        .to_string();

    assert!(
        tokens.contains("rate_limited_by_default : true"),
        "a procedure without @no_rate_limit should default its REST route to \
         rate_limited_by_default: true, got: {tokens}",
    );
}

#[test]
fn model_crud_routes_are_always_rate_limited_by_default() {
    let model = parse_first_model(MODEL_SCHEMA);

    let tokens = generate_model_transport_constants(&model).to_string();
    let occurrences = tokens.matches("rate_limited_by_default : true").count();

    assert_eq!(
        occurrences, 5,
        "all five CRUD routes (list/create/get/update/delete) should carry \
         rate_limited_by_default: true, got tokens: {tokens}",
    );
    assert!(
        !tokens.contains("rate_limited_by_default : false"),
        "model CRUD has no @no_rate_limit-equivalent opt-out today, got: {tokens}",
    );
}

// cratestack#743, `docs/design/route-suppression.md` §1.1: a suppressed
// verb's `RouteTransportDescriptor` const must not be emitted, and the
// entry list feeding `ROUTE_TRANSPORTS` must not reference it either —
// the design doc names this as "a second place any fix has to touch"
// alongside `axum/model/routes.rs` and `transport/op_descriptors.rs`.
#[test]
fn internal_verb_gets_no_route_transport_const() {
    let model = parse_first_model(MODEL_WITH_INTERNAL_CREATE_SCHEMA);

    let tokens = generate_model_transport_constants(&model).to_string();

    assert!(
        !tokens.contains("MODEL_WIDGET_LIST_POST"),
        "an @@internal(\"create\") model must not emit a RouteTransportDescriptor const for \
         POST /widgets, got: {tokens}",
    );
    // The other four survive untouched.
    assert!(tokens.contains("MODEL_WIDGET_LIST_GET"), "got: {tokens}");
    assert!(tokens.contains("MODEL_WIDGET_DETAIL_GET"), "got: {tokens}");
    assert!(
        tokens.contains("MODEL_WIDGET_DETAIL_PATCH"),
        "got: {tokens}"
    );
    assert!(
        tokens.contains("MODEL_WIDGET_DETAIL_DELETE"),
        "got: {tokens}"
    );
}

#[test]
fn internal_verb_gets_no_route_transport_entry() {
    let model = parse_first_model(MODEL_WITH_INTERNAL_CREATE_SCHEMA);

    let entries = generate_model_transport_entries(&model);
    let names: Vec<String> = entries.iter().map(|entry| entry.to_string()).collect();

    assert_eq!(
        entries.len(),
        4,
        "an @@internal(\"create\") model should contribute 4 ROUTE_TRANSPORTS entries, not 5, \
         got: {names:?}",
    );
    assert!(
        !names.iter().any(|name| name == "MODEL_WIDGET_LIST_POST"),
        "got: {names:?}",
    );
}

#[test]
fn fully_suppressed_model_gets_no_route_transport_entries_or_consts() {
    let schema = r#"
model Widget {
  id Int @id

  @@internal("all")
}
"#;
    let model = parse_first_model(schema);

    let entries = generate_model_transport_entries(&model);
    assert!(
        entries.is_empty(),
        "an @@internal(\"all\") model should contribute zero ROUTE_TRANSPORTS entries, got: {:?}",
        entries.iter().map(|e| e.to_string()).collect::<Vec<_>>()
    );

    let tokens = generate_model_transport_constants(&model).to_string();
    assert!(
        tokens.trim().is_empty(),
        "an @@internal(\"all\") model should emit no RouteTransportDescriptor consts at all, \
         got: {tokens}",
    );
}
