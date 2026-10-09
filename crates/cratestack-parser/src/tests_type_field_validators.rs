#![cfg(test)]
//! ADR 0019 D5 (PR A): a `type` field accepts the validator family because
//! the macros enforce it on procedure arguments
//! (`cratestack-macros/src/validators/types.rs`), and refuses what has no
//! reader there: `@default`, `@db_enforce`, and a validator on a list.

use super::parse_schema;

fn type_with(field: &str) -> String {
    format!("type T {{\n  f {field}\n}}\nprocedure p(args: T): T\n")
}

#[track_caller]
fn refused(field: &str) -> String {
    match parse_schema(&type_with(field)) {
        Ok(_) => panic!("`{field}` on a type field must be refused"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn every_validator_is_accepted_on_a_type_field_of_its_scalar() {
    for field in [
        "String @length(min: 3, max: 10)",
        "Bytes @length(min: 32, max: 32)",
        "Int @range(min: 0, max: 100)",
        "String @regex(\"^[a-z]+$\")",
        "String @email",
        "String @uri",
        "String @iso4217",
        "String? @length(min: 1)",
        "Int? @range(min: 1)",
    ] {
        parse_schema(&type_with(field)).unwrap_or_else(|e| panic!("`{field}` refused: {e}"));
    }
    // A validated `type` nests, optionally and as a list, with no
    // attribute on the field that holds it.
    parse_schema(
        "type Tag {\n  label String @length(min: 2)\n}\ntype Owner {\n  tags Tag[]\n  backup \
         Tag?\n  first Tag\n}\nprocedure p(args: Owner): Owner\n",
    )
    .expect("nested validated types");
}

/// The same scalar rules a model field has: the attribute must fit the type.
#[test]
fn a_validator_on_the_wrong_scalar_is_refused_as_on_a_model() {
    for (field, needle) in [
        (
            "Int @length(min: 1)",
            "@length on `T.f` is only valid on String or Bytes",
        ),
        (
            "String @range(min: 1)",
            "@range on `T.f` is only valid on Int or Decimal",
        ),
        (
            "Int @regex(\"a\")",
            "@regex on `T.f` is only valid on String",
        ),
        ("Int @email", "@email on `T.f` is only valid on String"),
        ("Bytes @uri", "@uri on `T.f` is only valid on String"),
        ("String @length", "@length requires arguments"),
        ("String @email(x)", "@email does not take arguments"),
    ] {
        let message = refused(field);
        assert!(message.contains(needle), "`{field}`: {message}");
    }
    let message = parse_schema(
        "type Tag {\n  label String\n}\ntype T {\n  f Tag @length(min: 1)\n}\nprocedure p(args: T): T\n",
    )
    .expect_err("a validator on a nested type field");
    assert!(
        message
            .to_string()
            .contains("only valid on String or Bytes"),
        "{message}"
    );
}

/// No reader applies a `@default` on a `type`: the generated decode fails
/// on a missing field (`cratestack-api/tests/contract_roundtrip.rs`).
#[test]
fn default_on_a_type_field_is_refused_and_points_at_optional() {
    for field in [
        "Int @default(1)",
        "String @default(\"x\")",
        "Boolean @default(false)",
    ] {
        let message = refused(field);
        assert!(message.contains("`T` is a `type`"), "{message}");
        assert!(
            message.contains("a missing field is a decode error"),
            "{message}"
        );
        assert!(message.contains("Make the field optional"), "{message}");
    }
}

/// A `type` has no table, so there is no `CHECK` for `@db_enforce` to add.
#[test]
fn db_enforce_on_a_type_field_is_refused_with_the_reason() {
    for field in [
        "String @db_enforce @length(min: 1)",
        "String @length(min: 1) @db_enforce",
    ] {
        let message = refused(field);
        assert!(
            message.contains("`T` is a `type`, which has no table"),
            "{message}"
        );
        assert!(message.contains("database CHECK constraint"), "{message}");
        assert!(message.contains("Remove `@db_enforce`"), "{message}");
    }
    // The same attribute stays valid on a model field.
    parse_schema("model M {\n  id Int @id\n  n Int @range(min: 0) @db_enforce\n}\n")
        .expect("@db_enforce on a model field");
}

#[test]
fn a_validator_on_a_list_field_is_refused_and_a_list_of_a_validated_type_is_the_way() {
    for field in [
        "String[] @length(max: 3)",
        "Int[] @range(min: 0)",
        "String[] @regex(\"a\")",
        "String[] @email",
        "String[] @uri",
        "String[] @iso4217",
    ] {
        let message = refused(field);
        assert!(message.contains("is a list"), "`{field}`: {message}");
        assert!(
            message.contains("take a list of that `type`"),
            "`{field}`: {message}"
        );
    }
}

/// What a `type` field still refuses is unchanged: the rest of the model list.
#[test]
fn the_other_model_attributes_are_still_refused_on_a_type_field() {
    for name in [
        "@pii",
        "@sensitive",
        "@unique",
        "@readonly",
        "@version",
        "@id",
    ] {
        let message = refused(&format!("String {name}"));
        assert!(
            message.contains("unsupported attribute"),
            "{name}: {message}"
        );
        assert!(
            message.contains("`@iso4217`, `@length`, `@range`, `@regex`"),
            "{name}: {message}"
        );
    }
}
