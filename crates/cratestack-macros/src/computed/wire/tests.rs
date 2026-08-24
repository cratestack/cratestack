//! Proves [`super::generate_wire_structs`]'s recursive substitution: a
//! wire struct's field that names another computed-bearing owner must
//! resolve to *that* owner's own wire struct, not the plain server-side
//! shape — the exact case `docs/design/computed-fields.md`'s follow-up
//! called out (`type Card { cover Image }`). Token-string assertions
//! (rather than compiling the output) keep these fast and DB-less, same
//! style as `crate::computed::bearing::tests`.

use quote::ToTokens;

use crate::shared::{enum_name_set, model_name_set};

use super::generate_wire_structs;

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("fixture schema should parse")
}

#[test]
fn nested_bearing_type_field_resolves_to_the_wire_struct() {
    let schema = schema(
        "type Image {\n  storageKey String\n  thumbnailUrl String @computed\n}\n\
         type Card {\n  cover Image\n}\n",
    );
    let bearing = crate::computed::computed_bearing_names(&schema);
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);

    let structs = generate_wire_structs(&schema, &model_names, &enum_names, &bearing);
    let rendered = structs
        .iter()
        .map(|ts| ts.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("struct Card"),
        "wire module should emit a Card struct: {rendered}"
    );
    assert!(
        rendered.contains("cover : super :: wire :: Image"),
        "Card.cover must resolve to the sibling wire::Image struct, not \
         plain super::Image (the server-side shape, which drops computed \
         fields) — rendered: {rendered}"
    );
    assert!(
        !rendered.contains("cover : super :: Image ,"),
        "Card.cover must not resolve to the plain server-side super::Image \
         — rendered: {rendered}"
    );
}

#[test]
fn nested_bearing_model_field_on_a_type_resolves_to_the_wire_struct() {
    let schema = schema(
        "model Photo {\n  id Int @id\n  storageKey String\n  proxyUrl String @computed\n}\n\
         type Gallery {\n  cover Photo\n}\n",
    );
    let bearing = crate::computed::computed_bearing_names(&schema);
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);

    let structs = generate_wire_structs(&schema, &model_names, &enum_names, &bearing);
    let rendered = structs
        .iter()
        .map(|ts| ts.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("cover : super :: wire :: Photo"),
        "a `type` field nesting a bearing `model` directly must also \
         resolve to the sibling wire struct — rendered: {rendered}"
    );
}

#[test]
fn non_bearing_field_keeps_the_plain_server_side_path() {
    let schema = schema(
        "type Image {\n  storageKey String\n  thumbnailUrl String @computed\n}\n\
         type Meta {\n  label String\n}\n\
         type Card {\n  cover Image\n  meta Meta\n}\n",
    );
    let bearing = crate::computed::computed_bearing_names(&schema);
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);

    let structs = generate_wire_structs(&schema, &model_names, &enum_names, &bearing);
    let rendered = structs
        .iter()
        .map(|ts| ts.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("meta : super :: Meta"),
        "a non-bearing nested type keeps the plain super:: path — \
         nothing to lose to the wire redirection: {rendered}"
    );
    assert!(!rendered.contains("wire :: Meta"));
}

#[test]
fn no_computed_fields_emits_no_wire_structs() {
    let schema = schema("type Plain {\n  label String\n}\n");
    let bearing = crate::computed::computed_bearing_names(&schema);
    assert!(bearing.is_empty());

    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let structs = generate_wire_structs(&schema, &model_names, &enum_names, &bearing);

    assert!(
        structs.is_empty(),
        "a computed-free schema must not emit anything into the wire module"
    );
}
