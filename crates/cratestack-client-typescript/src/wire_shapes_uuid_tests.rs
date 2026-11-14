// cratestack#1359: `Uuid` rows of `build_wire_shapes` and `procedure_revival`.
// Child module of `wire_shapes_tests`, whose builders it uses.

use cratestack_core::{Attribute, TypeArity, TypeRef};

use super::{
    ProcedureRevival, ScalarRevival, build_wire_shapes, field, model, procedure_revival, schema,
    span,
};

#[test]
fn uuid_fields_are_one_key_list_at_every_arity() {
    let schema = schema(
        vec![model(
            "Doc",
            vec![
                field("id", "Uuid", TypeArity::Required),
                field("parentId", "Uuid", TypeArity::Optional),
                field("tagIds", "Uuid", TypeArity::List),
                field("title", "String", TypeArity::Required),
            ],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let doc = shapes.iter().find(|s| s.name == "Doc").unwrap();
    assert_eq!(doc.uuid_keys_js, "['id', 'parentId', 'tagIds']");
    // A `Uuid` is never a `Bytes`, `BigInt` or `Decimal` key.
    assert_eq!(doc.bytes_keys_js, "[]");
    assert_eq!(doc.bytes_list_keys_js, "[]");
    assert_eq!(doc.bigint_keys_js, "[]");
    assert_eq!(doc.decimal_keys_js, "[]");
}

#[test]
fn a_uuid_field_and_a_same_named_string_field_get_independent_shapes() {
    let schema = schema(
        vec![
            model("Doc", vec![field("ref", "Uuid", TypeArity::Required)]),
            model("Note", vec![field("ref", "String", TypeArity::Required)]),
        ],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    assert_eq!(
        shapes.iter().find(|s| s.name == "Doc").unwrap().uuid_keys_js,
        "['ref']"
    );
    assert_eq!(
        shapes.iter().find(|s| s.name == "Note").unwrap().uuid_keys_js,
        "[]"
    );
}

#[test]
fn server_only_uuid_fields_get_no_revival_entry() {
    let mut hidden = field("secret", "Uuid", TypeArity::Required);
    hidden.attributes.push(Attribute {
        raw: "@server_only".to_owned(),
        span: span(),
    });
    let schema = schema(
        vec![model(
            "Vault",
            vec![hidden, field("public", "Uuid", TypeArity::Required)],
        )],
        Vec::new(),
    );
    let shapes = build_wire_shapes(&schema);
    let vault = shapes.iter().find(|s| s.name == "Vault").unwrap();
    assert_eq!(vault.uuid_keys_js, "['public']");
}

#[test]
fn bare_uuid_returns_are_a_scalar_uuid_revival_at_every_arity() {
    for arity in [TypeArity::Required, TypeArity::Optional, TypeArity::List] {
        let ty = TypeRef {
            name: "Uuid".to_owned(),
            name_span: span(),
            arity,
            generic_args: Vec::new(),
            int_args: Vec::new(),
            ident_args: Vec::new(),
        };
        assert!(
            matches!(
                procedure_revival(&ty),
                ProcedureRevival::Scalar(ScalarRevival::Uuid)
            ),
            "{arity:?}"
        );
    }
}
