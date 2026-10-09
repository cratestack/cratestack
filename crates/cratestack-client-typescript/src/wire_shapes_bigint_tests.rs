// ADR 0019: `BigInt` rows of `build_wire_shapes`. Child module of
// `wire_shapes_tests`, whose `schema`/`model`/`field`/`span` builders it uses.

use cratestack_core::TypeArity;

use super::{build_wire_shapes, field, model, schema, span};

#[test]
fn bigint_fields_are_one_key_list_at_every_arity() {
    let schema = schema(
        vec![model(
            "Ledger",
            vec![
                field("amountE8", "BigInt", TypeArity::Required),
                field("feeE8", "BigInt", TypeArity::Optional),
                field("tiers", "BigInt", TypeArity::List),
                field("note", "String", TypeArity::Required),
            ],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let ledger = shapes.iter().find(|s| s.name == "Ledger").unwrap();
    assert_eq!(ledger.bigint_keys_js, "['amountE8', 'feeE8', 'tiers']");
    // A `BigInt` is never a `Decimal` or `Bytes` key, and vice versa.
    assert_eq!(ledger.decimal_keys_js, "[]");
    assert_eq!(ledger.bytes_keys_js, "[]");
    assert_eq!(ledger.bytes_list_keys_js, "[]");
}

#[test]
fn a_bigint_field_and_a_same_named_string_field_get_independent_shapes() {
    // The by-type scheme is the whole point (see the module doc): `Entry.amountE8`
    // is a `String` and must never be revived because `Ledger.amountE8` is a
    // `BigInt`.
    let schema = schema(
        vec![
            model(
                "Ledger",
                vec![field("amountE8", "BigInt", TypeArity::Required)],
            ),
            model(
                "Entry",
                vec![field("amountE8", "String", TypeArity::Required)],
            ),
        ],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let get = |name: &str| shapes.iter().find(|s| s.name == name).unwrap();
    assert_eq!(get("Ledger").bigint_keys_js, "['amountE8']");
    assert_eq!(get("Entry").bigint_keys_js, "[]");
}

#[test]
fn a_bigint_in_a_type_decl_and_behind_a_relation_is_routed_not_flattened() {
    let schema = schema(
        vec![model(
            "Order",
            vec![
                field("totals", "Totals", TypeArity::Required),
                field("customer", "Customer", TypeArity::Required),
            ],
        )],
        vec![cratestack_core::TypeDecl {
            docs: Vec::new(),
            name: "Totals".to_owned(),
            name_span: span(),
            fields: vec![field("grossE8", "BigInt", TypeArity::Required)],
            span: span(),
        }],
    );
    let shapes = build_wire_shapes(&schema);
    let order = shapes.iter().find(|s| s.name == "Order").unwrap();
    assert_eq!(order.bigint_keys_js, "[]");
    assert!(
        order.nested_js.contains("'totals': 'Totals'"),
        "{}",
        order.nested_js
    );
    let totals = shapes.iter().find(|s| s.name == "Totals").unwrap();
    assert_eq!(totals.bigint_keys_js, "['grossE8']");
}

#[test]
fn server_only_bigint_fields_get_no_revival_entry() {
    let mut hidden = field("secretE8", "BigInt", TypeArity::Required);
    hidden.attributes.push(cratestack_core::Attribute {
        raw: "@server_only".to_owned(),
        span: span(),
    });
    let schema = schema(
        vec![model(
            "Vault",
            vec![hidden, field("publicE8", "BigInt", TypeArity::Required)],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let vault = shapes.iter().find(|s| s.name == "Vault").unwrap();
    assert_eq!(vault.bigint_keys_js, "['publicE8']");
}
