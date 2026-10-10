//! Auth-derived create defaults for a `BigInt` field (ADR 0019 D3, PR B), and
//! the guard on the scalar table's catch-all.

use cratestack_core::Schema;

use super::collect_create_defaults;
use crate::shared::test_support::builtin_scalars;

const SCHEMA: &str = r#"
auth Operator {
  accountId BigInt
}

model Account {
  id Int @id
  ownerId BigInt @default(auth().accountId)
  @@allow("all", auth() != null)
}
"#;

fn schema() -> Schema {
    cratestack_parser::parse_schema(SCHEMA).expect("fixture parses")
}

fn defaults(schema: &Schema) -> Result<Vec<String>, String> {
    collect_create_defaults(
        &schema.models[0],
        &schema.models,
        &schema.types,
        schema.auth.as_ref(),
    )
    .map(|all| all.iter().map(ToString::to_string).collect())
}

#[test]
fn a_bigint_default_is_the_bigint_kind_and_names_its_claim() {
    let rendered = defaults(&schema()).expect("a BigInt default generates")[0].clone();
    assert!(rendered.contains("column : \"owner_id\""), "{rendered}");
    assert!(
        rendered.contains("auth_field : \"accountId\""),
        "{rendered}"
    );
    assert!(
        rendered.contains("ty : :: cratestack :: CreateDefaultType :: BigInt"),
        "an `Int` kind would bind the claim as the wrong variant: {rendered}"
    );
    assert!(rendered.contains("nullable : false"), "{rendered}");
}

#[test]
fn an_optional_bigint_default_is_nullable() {
    let mut schema = schema();
    schema.models[0].fields[1].ty.arity = cratestack_core::TypeArity::Optional;
    let rendered = defaults(&schema).expect("generates")[0].clone();
    assert!(rendered.contains("nullable : true"), "{rendered}");
}

#[test]
fn a_bigint_default_needs_a_bigint_claim() {
    let mut schema = schema();
    schema.auth.as_mut().expect("auth block").fields[0].ty.name = "Int".to_owned();
    let error = defaults(&schema).expect_err("an Int claim does not fill a BigInt column");
    assert!(error.contains("matching auth/model field types"), "{error}");
}

#[test]
fn exactly_the_documented_scalars_support_an_auth_derived_default() {
    // Guard on the `other => Err(..)` arm: a built-in scalar either has a
    // `CreateDefaultType` or is refused with the supported list.
    let supported = ["String", "Cuid", "Int", "BigInt", "Boolean"];
    for scalar in builtin_scalars() {
        let mut schema = schema();
        schema.models[0].fields[1].ty.name = scalar.to_owned();
        schema.auth.as_mut().expect("auth block").fields[0].ty.name = scalar.to_owned();
        let result = defaults(&schema);
        assert_eq!(
            result.is_ok(),
            supported.contains(&scalar),
            "`{scalar}`: auth-derived default support changed ({result:?}); add a \
             `CreateDefaultType` arm and update the error text, or leave it refused"
        );
        if let Err(error) = result {
            assert!(
                error.contains("BigInt"),
                "the refusal lists the supported kinds: {error}"
            );
        }
    }
}
