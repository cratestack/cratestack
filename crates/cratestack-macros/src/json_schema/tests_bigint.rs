//! `BigInt` (ADR 0019) in every position a schema can reach it: as a
//! procedure argument, as a field of a `type` and of a `model`, and in every
//! arity. The fragment is a string with the canonical decimal pattern and
//! never an `integer`. Whether it agrees with `cratestack::BigInt`'s serde,
//! for `i64::MAX`, `i64::MIN`, `2^53 + 1` and a JSON number, is the
//! round-trip suites' job (`cratestack-api` and `cratestack-pg`
//! `tests/json_schema_*.rs`).

use serde_json::{Value, json};

use super::tests::parse;
use super::{procedure_input_schema, procedure_output_schema};
use crate::shared::decimal_backend::DecimalBackend;

/// `BigInt`'s fragment, written out in full on purpose: a change to the
/// pattern or a stray `format` or `minimum` must fail here by name.
fn fragment() -> Value {
    json!({ "type": "string", "pattern": "^(0|-?[1-9][0-9]{0,18})$" })
}

const DECLS: &str = "type Ledger {\n  total BigInt\n  maybe BigInt?\n  totals BigInt[]\n}\n\
    model Account {\n  id BigInt @id\n  balance BigInt\n  limit BigInt?\n}\n";

fn input(source: &str) -> Value {
    let schema = parse(source);
    procedure_input_schema(
        &schema,
        &schema.procedures[0],
        Some(DecimalBackend::RustDecimal),
    )
    .expect("input schema")
}

fn output(source: &str) -> Value {
    let schema = parse(source);
    procedure_output_schema(&schema, &schema.procedures[0], None)
        .expect("output schema")
        .expect("an object return type")
}

/// No `integer` anywhere: a schema that said so would let an agent send a
/// JSON number, which the server refuses.
fn assert_never_an_integer(schema: &Value, context: &str) {
    let text = schema.to_string();
    assert!(!text.contains("integer"), "{context}: {text}");
}

#[test]
fn a_procedure_argument_is_a_canonical_decimal_string() {
    let schema = input(&format!(
        "{DECLS}procedure p(total: BigInt, maybe: BigInt?, totals: BigInt[]): Boolean"
    ));
    let properties = &schema["properties"];
    assert_eq!(properties["total"], fragment());
    assert_eq!(
        properties["maybe"],
        json!({ "anyOf": [fragment(), { "type": "null" }] })
    );
    assert_eq!(
        properties["totals"],
        json!({ "type": "array", "items": fragment() })
    );
    assert_eq!(schema["required"], json!(["total", "totals"]));
    assert_never_an_integer(&schema, "procedure arguments");
}

#[test]
fn type_and_model_fields_are_canonical_decimal_strings() {
    let schema = input(&format!(
        "{DECLS}procedure p(ledger: Ledger, account: Account): Boolean"
    ));
    let ledger = &schema["$defs"]["Ledger"]["properties"];
    assert_eq!(ledger["total"], fragment());
    assert_eq!(
        ledger["maybe"],
        json!({ "anyOf": [fragment(), { "type": "null" }] })
    );
    assert_eq!(
        ledger["totals"],
        json!({ "type": "array", "items": fragment() })
    );
    let account = &schema["$defs"]["Account"];
    assert_eq!(account["properties"]["id"], fragment());
    assert_eq!(account["properties"]["balance"], fragment());
    assert_eq!(
        account["properties"]["limit"],
        json!({ "anyOf": [fragment(), { "type": "null" }] })
    );
    assert_eq!(account["required"], json!(["id", "balance"]));
    assert_never_an_integer(&schema, "type and model fields");
}

#[test]
fn returns_and_page_items_are_canonical_decimal_strings() {
    let ledger = output(&format!("{DECLS}procedure p(): Ledger"));
    assert_eq!(ledger["$defs"]["Ledger"]["properties"]["total"], fragment());
    assert_never_an_integer(&ledger, "type return");

    let account = output(&format!("{DECLS}procedure p(): Account"));
    assert_eq!(account["$defs"]["Account"]["properties"]["id"], fragment());
    assert_never_an_integer(&account, "model return");

    // `Page<T>` is the one place an `integer` is right: its counters are
    // framework `i64`s. The `BigInt` it carries is still a string.
    let page = output(&format!("{DECLS}procedure p(): Page<Account>"));
    assert_eq!(page["$defs"]["Account"]["properties"]["id"], fragment());
    assert_eq!(
        page["properties"]["totalCount"]["anyOf"][0]["type"],
        "integer"
    );
    assert_eq!(
        page["$defs"]["Account"]["properties"]["balance"],
        fragment()
    );
}

#[test]
fn big_int_and_int_stay_distinct() {
    let schema = input("procedure p(small: Int, big: BigInt): Boolean");
    let properties = &schema["properties"];
    assert_eq!(properties["small"]["type"], "integer");
    assert_eq!(properties["big"], fragment());
    assert_ne!(properties["small"], properties["big"]);
}
