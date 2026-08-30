// Unit tests for `build_wire_shapes` — split into its own file via
// `#[path]` to keep `wire_shapes.rs` itself under this repo's ~200-LoC
// convention, mirroring `swr::ownership`/`ownership_tests.rs`'s split.

use cratestack_core::{Schema, TransportStyle, TypeArity};

use super::{ProcedureRevival, ScalarRevival, build_wire_shapes, procedure_revival};

fn schema(models: Vec<cratestack_core::Model>, types: Vec<cratestack_core::TypeDecl>) -> Schema {
    Schema {
        datasource: None,
        auth: None,
        config_blocks: Vec::new(),
        mixins: Vec::new(),
        models,
        types,
        enums: Vec::new(),
        procedures: Vec::new(),
        views: Vec::new(),
        transport: TransportStyle::Rest,
        declared_extensions: Default::default(),
    }
}

fn span() -> cratestack_core::SourceSpan {
    cratestack_core::SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

fn field(name: &str, ty_name: &str, arity: TypeArity) -> cratestack_core::Field {
    cratestack_core::Field {
        docs: Vec::new(),
        name: name.to_owned(),
        name_span: span(),
        ty: cratestack_core::TypeRef {
            name: ty_name.to_owned(),
            name_span: span(),
            arity,
            generic_args: Vec::new(),
            int_args: Vec::new(),
            ident_args: Vec::new(),
        },
        attributes: Vec::new(),
        span: span(),
    }
}

fn model(name: &str, fields: Vec<cratestack_core::Field>) -> cratestack_core::Model {
    cratestack_core::Model {
        docs: Vec::new(),
        name: name.to_owned(),
        name_span: span(),
        fields,
        attributes: Vec::new(),
        span: span(),
    }
}

#[test]
fn direct_field_becomes_a_key_not_a_nested_entry() {
    let schema = schema(
        vec![model(
            "Invoice",
            vec![field("amount", "Decimal", TypeArity::Required)],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let invoice = shapes.iter().find(|s| s.name == "Invoice").unwrap();
    assert_eq!(invoice.decimal_keys_js, "['amount']");
    assert_eq!(invoice.nested_js, "{  }");
}

#[test]
fn relation_field_becomes_a_nested_entry_not_a_key() {
    let schema = schema(
        vec![
            model(
                "Customer",
                vec![field("balance", "Decimal", TypeArity::Required)],
            ),
            model(
                "Invoice",
                vec![
                    field("amount", "Decimal", TypeArity::Required),
                    field("customer", "Customer", TypeArity::Required),
                ],
            ),
        ],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let invoice = shapes.iter().find(|s| s.name == "Invoice").unwrap();
    assert_eq!(invoice.decimal_keys_js, "['amount']");
    assert_eq!(invoice.nested_js, "{ 'customer': 'Customer' }");
    let customer = shapes.iter().find(|s| s.name == "Customer").unwrap();
    assert_eq!(customer.decimal_keys_js, "['balance']");
}

#[test]
fn same_named_field_in_two_types_gets_two_independent_shapes() {
    // The exact collision shape this module exists to rule out:
    // `Order.total: Decimal`, related `Account.total: String`. Each
    // type's shape only ever describes its own fields.
    let schema = schema(
        vec![
            model(
                "Account",
                vec![field("total", "String", TypeArity::Required)],
            ),
            model(
                "Order",
                vec![
                    field("total", "Decimal", TypeArity::Required),
                    field("account", "Account", TypeArity::Required),
                ],
            ),
        ],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let order = shapes.iter().find(|s| s.name == "Order").unwrap();
    assert_eq!(order.decimal_keys_js, "['total']");
    let account = shapes.iter().find(|s| s.name == "Account").unwrap();
    assert_eq!(
        account.decimal_keys_js, "[]",
        "Account.total is a String, not a Decimal — must not appear as a key"
    );
}

#[test]
fn self_referential_relation_is_a_nested_entry_pointing_at_its_own_shape() {
    let schema = schema(
        vec![model(
            "Task",
            vec![
                field("cost", "Decimal", TypeArity::Required),
                field("parent", "Task", TypeArity::Optional),
            ],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let task = shapes.iter().find(|s| s.name == "Task").unwrap();
    assert_eq!(task.decimal_keys_js, "['cost']");
    assert_eq!(task.nested_js, "{ 'parent': 'Task' }");
}

#[test]
fn type_decl_gets_a_shape_like_a_model() {
    let ty = cratestack_core::TypeDecl {
        docs: Vec::new(),
        name: "QuoteResult".to_owned(),
        name_span: span(),
        fields: vec![field("price", "Decimal", TypeArity::Required)],
        span: span(),
    };
    let shapes = build_wire_shapes(&schema(Vec::new(), vec![ty]));
    let quote_result = shapes.iter().find(|s| s.name == "QuoteResult").unwrap();
    assert_eq!(quote_result.decimal_keys_js, "['price']");
}

#[test]
fn bytes_fields_are_split_by_arity() {
    // The arity split this module's doc explains: a populated `Bytes` and
    // a populated `Bytes[]` are structurally distinguishable at runtime
    // (`number[]` vs `number[][]`), but `[]` is not — it is either an
    // empty `Uint8Array` or an empty list of them. Recording the arity
    // here is what removes the guess.
    let schema = schema(
        vec![model(
            "Blob",
            vec![
                field("digest", "Bytes", TypeArity::Required),
                field("signature", "Bytes", TypeArity::Optional),
                field("chunks", "Bytes", TypeArity::List),
            ],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let blob = shapes.iter().find(|s| s.name == "Blob").unwrap();
    assert_eq!(blob.bytes_keys_js, "['digest', 'signature']");
    assert_eq!(blob.bytes_list_keys_js, "['chunks']");
    assert_eq!(
        blob.decimal_keys_js, "[]",
        "a Bytes field must never land in the Decimal key set"
    );
}

#[test]
fn a_bytes_field_and_an_int_list_field_are_told_apart_by_the_schema_not_the_wire() {
    // Both decode to `number[]` on the wire — indistinguishable without
    // the schema, which is the whole reason `Bytes` needs this registry
    // rather than a structural guess (exactly the argument that already
    // applies to `Decimal` vs `String`).
    let schema = schema(
        vec![model(
            "Sample",
            vec![
                field("payload", "Bytes", TypeArity::Required),
                field("readings", "Int", TypeArity::List),
            ],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let sample = shapes.iter().find(|s| s.name == "Sample").unwrap();
    assert_eq!(sample.bytes_keys_js, "['payload']");
    assert_eq!(
        sample.bytes_list_keys_js, "[]",
        "an Int[] field must not be revived as a list of byte arrays"
    );
}

#[test]
fn decimal_and_bytes_coexist_in_one_shape() {
    // One walk, two conversions — the reason `Bytes` piggy-backs on this
    // registry instead of getting a parallel one.
    let schema = schema(
        vec![model(
            "Receipt",
            vec![
                field("amount", "Decimal", TypeArity::Required),
                field("seal", "Bytes", TypeArity::Required),
            ],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let receipt = shapes.iter().find(|s| s.name == "Receipt").unwrap();
    assert_eq!(receipt.decimal_keys_js, "['amount']");
    assert_eq!(receipt.bytes_keys_js, "['seal']");
}

#[test]
fn server_only_bytes_fields_get_no_revival_entry() {
    // `@server_only` is masked from every outbound response, so it can
    // never appear in a decoded body — same reasoning the Decimal keys
    // already apply.
    let mut hidden = field("secret", "Bytes", TypeArity::Required);
    hidden.attributes.push(cratestack_core::Attribute {
        raw: "@server_only".to_owned(),
        span: span(),
    });
    let schema = schema(
        vec![model(
            "Vault",
            vec![hidden, field("public", "Bytes", TypeArity::Required)],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let vault = shapes.iter().find(|s| s.name == "Vault").unwrap();
    assert_eq!(vault.bytes_keys_js, "['public']");
}

#[test]
fn procedure_scalar_returns_are_classified_by_type_and_arity() {
    fn kind(name: &str, arity: TypeArity) -> String {
        let return_type = cratestack_core::TypeRef {
            name: name.to_owned(),
            name_span: span(),
            arity,
            generic_args: Vec::new(),
            int_args: Vec::new(),
            ident_args: Vec::new(),
        };
        match procedure_revival(&return_type) {
            ProcedureRevival::Scalar(scalar) => scalar.as_str().to_owned(),
            ProcedureRevival::Shape { shape_name, .. } => format!("shape:{shape_name}"),
        }
    }

    assert_eq!(kind("Decimal", TypeArity::Required), "decimal");
    assert_eq!(kind("Decimal", TypeArity::List), "decimal");
    assert_eq!(kind("Bytes", TypeArity::Required), "bytes");
    assert_eq!(kind("Bytes", TypeArity::Optional), "bytes");
    // The one case that needs its own kind: a list of byte arrays is
    // `number[][]` on the wire, not one byte array.
    assert_eq!(kind("Bytes", TypeArity::List), "bytesList");
    // Everything else routes through the shape registry, including a
    // scalar with no entry (a documented no-op at runtime).
    assert_eq!(kind("String", TypeArity::Required), "shape:String");
    assert_eq!(kind("Invoice", TypeArity::Required), "shape:Invoice");
}

#[test]
fn scalar_revival_strings_match_the_runtime_contract() {
    // These exact strings are rendered into the generated
    // `reviveWireScalar(value, "...")` call, and `models.ts.j2`'s runtime
    // switches on them. Drift here is a silent no-op revival, not a
    // compile error, so pin them.
    assert_eq!(ScalarRevival::Decimal.as_str(), "decimal");
    assert_eq!(ScalarRevival::Bytes.as_str(), "bytes");
    assert_eq!(ScalarRevival::BytesList.as_str(), "bytesList");
}
