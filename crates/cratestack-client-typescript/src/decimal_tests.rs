// Unit tests for `build_decimal_shapes` — split into its own file via
// `#[path]` to keep `decimal.rs` itself under this repo's ~200-LoC
// convention, mirroring `swr::ownership`/`ownership_tests.rs`'s split.

use cratestack_core::{Schema, TransportStyle, TypeArity};

use super::build_decimal_shapes;

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
    let shapes = build_decimal_shapes(&schema);
    let invoice = shapes.iter().find(|s| s.name == "Invoice").unwrap();
    assert_eq!(invoice.keys_js, "['amount']");
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
    let shapes = build_decimal_shapes(&schema);
    let invoice = shapes.iter().find(|s| s.name == "Invoice").unwrap();
    assert_eq!(invoice.keys_js, "['amount']");
    assert_eq!(invoice.nested_js, "{ 'customer': 'Customer' }");
    let customer = shapes.iter().find(|s| s.name == "Customer").unwrap();
    assert_eq!(customer.keys_js, "['balance']");
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
    let shapes = build_decimal_shapes(&schema);
    let order = shapes.iter().find(|s| s.name == "Order").unwrap();
    assert_eq!(order.keys_js, "['total']");
    let account = shapes.iter().find(|s| s.name == "Account").unwrap();
    assert_eq!(
        account.keys_js, "[]",
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
    let shapes = build_decimal_shapes(&schema);
    let task = shapes.iter().find(|s| s.name == "Task").unwrap();
    assert_eq!(task.keys_js, "['cost']");
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
    let shapes = build_decimal_shapes(&schema(Vec::new(), vec![ty]));
    let quote_result = shapes.iter().find(|s| s.name == "QuoteResult").unwrap();
    assert_eq!(quote_result.keys_js, "['price']");
}
