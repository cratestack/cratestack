//! Regression test for issue #137 on the embedded (rusqlite) composer —
//! see crates/cratestack-pg/tests/type_block_model_reference.rs for the
//! server-composer equivalent. Both share `generate_type_struct`.

use cratestack::include_embedded_schema;

include_embedded_schema!("tests/fixtures/type_references_model.cstack");

#[test]
fn type_block_field_referencing_a_model_compiles_and_constructs() {
    let secret = cratestack_schema::ApiKeySecret {
        model: cratestack_schema::SomeModel {
            id: 1,
            name: "primary".to_owned(),
        },
        secret: "shh".to_owned(),
    };

    assert_eq!(secret.model.id, 1);
    assert_eq!(secret.model.name, "primary");
    assert_eq!(secret.secret, "shh");
}
