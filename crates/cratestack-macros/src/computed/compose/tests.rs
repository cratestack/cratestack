//! What a compose helper reads off the struct. It bypasses serde, so the
//! field list it is built from is the only thing keeping a `@server_only`
//! value out of the response. Token-string assertions, fast and DB-less,
//! like `crate::computed::wire::tests`; `cratestack-pg`'s
//! `tests/server_only_outbound*.rs` prove the same over every transport.

use quote::ToTokens;

use crate::shared::model_name_set;

use super::generate_compose_helpers;

fn rendered(source: &str) -> String {
    let schema = cratestack_parser::parse_schema(source).expect("fixture schema should parse");
    let bearing = crate::computed::computed_bearing_names(&schema);
    let model_names = model_name_set(&schema.models);
    generate_compose_helpers(&schema, &model_names, &bearing)
        .iter()
        .map(|helper| helper.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_model_compose_helper_never_reads_a_server_only_field() {
    let rendered = rendered(
        "model Widget {\n  id Int @id\n  label String\n  secret String @server_only\n  \
         hint String @computed\n}\n",
    );

    assert!(
        rendered.contains("value . label . clone ()"),
        "the stored field is copied: {rendered}"
    );
    assert!(
        rendered.contains("resolve_widget_hint"),
        "the computed field is resolved: {rendered}"
    );
    assert!(
        !rendered.contains("secret"),
        "`@server_only` must be neither read nor named: {rendered}"
    );
}

/// A `type` that embeds the model composes it through the model's own
/// helper, so the model's field list is still the one that decides.
#[test]
fn a_type_embedding_the_model_composes_through_the_model_helper() {
    let rendered = rendered(
        "model Widget {\n  id Int @id\n  secret String @server_only\n  hint String @computed\n}\n\
         type Envelope {\n  one Widget\n  many Widget[]\n}\n",
    );

    assert!(
        rendered.contains("compose_widget_value (db , resolvers , ctx , & value . one)"),
        "{rendered}"
    );
    assert!(!rendered.contains("secret"), "{rendered}");
}
