//! Regression test for issue #137: a `type` block field referencing a
//! `model` type failed codegen with "cannot find type in scope". `type`
//! structs are generated into `cratestack_schema::types` while `model`
//! structs are generated into the sibling `cratestack_schema::models`
//! module, and the `type`-block field emitter never module-qualified a
//! model-typed field reference.

use cratestack::{include_client_schema, include_server_schema};

include_server_schema!("tests/fixtures/type_references_model.cstack", db = Postgres);

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

mod client_only_schema {
    use super::include_client_schema;

    include_client_schema!("tests/fixtures/type_references_model.cstack");

    #[test]
    fn client_type_block_field_referencing_a_model_compiles_and_constructs() {
        let secret = cratestack_schema::ApiKeySecret {
            model: cratestack_schema::SomeModel {
                id: 2,
                name: "client".to_owned(),
            },
            secret: "shh-client".to_owned(),
        };

        assert_eq!(secret.model.id, 2);
        assert_eq!(secret.model.name, "client");
        assert_eq!(secret.secret, "shh-client");
    }
}
