#![cfg(test)]
//! cratestack#1074: `@id` is matched exactly (not as a prefix), `@id(...)`
//! is refused, and a second `@relation` on one field is refused.

use super::parse_schema;

#[test]
fn identity_is_not_a_primary_key() {
    // Before the closed list (ADR 0019 D5) `@identity` was inert and the
    // model failed for want of a key; now the attribute itself is refused.
    let error = parse_schema(
        r#"
model Account {
  code String @identity
  name String
}
"#,
    )
    .expect_err("`@identity` must not satisfy the primary-key requirement");

    let message = error.to_string();
    assert!(
        message.contains("unsupported attribute `@identity` on a model field"),
        "{message}",
    );
    assert!(!message.contains("did you mean `@id`"), "{message}");
}

#[test]
fn id_prefixed_attributes_beside_a_real_id_are_refused_not_counted() {
    for name in ["@identity", "@id_foo"] {
        let error = parse_schema(&format!(
            "model Account {{\n  id Int @id\n  code String {name}\n}}\n"
        ))
        .expect_err("an unknown `@id…` attribute is refused, not a second `@id`");
        let message = error.to_string();
        assert!(
            message.contains(&format!("unsupported attribute `{name}`")),
            "{message}"
        );
        assert!(!message.contains("more than one field-level"), "{message}");
    }
}

#[test]
fn idx_is_a_near_miss_of_id_not_a_second_id() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  slug String @idx
}
"#,
    )
    .expect_err("`@idx` is a near-miss of `@id`");

    let message = error.to_string();
    assert!(message.contains("did you mean `@id`?"), "{message}");
    assert!(!message.contains("more than one field-level"), "{message}");
}

#[test]
fn id_with_arguments_is_refused_at_the_attribute() {
    for raw in ["@id()", "@id(sort: Desc)"] {
        let source = format!("model Account {{\n  id Int {raw}\n}}\n");
        let error = parse_schema(&source).expect_err("`@id` takes no arguments");
        assert!(
            error.to_string().contains("`@id` does not take arguments"),
            "{error}",
        );
        let start = source.find(raw).expect("attribute in source");
        assert_eq!(error.span(), start..start + raw.len());
    }
}

const TWO_RELATIONS: &str = r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  editorId Int
  author User @relation(fields:[authorId],references:[id]) @relation(fields:[editorId],references:[id])
}
"#;

#[test]
fn a_second_relation_is_refused_at_the_second() {
    let error = parse_schema(TWO_RELATIONS).expect_err("two `@relation`s on one field");

    assert!(
        error
            .to_string()
            .contains("field `author` on model `Post` declares `@relation` more than once"),
        "{error}",
    );
    let second = "@relation(fields:[editorId],references:[id])";
    let start = TWO_RELATIONS
        .find(second)
        .expect("second relation in source");
    assert_eq!(error.span(), start..start + second.len());
}

#[test]
fn a_bare_relation_is_refused_before_it_can_count() {
    // `@relation` takes an argument list, so a bare one is refused by the
    // closed list itself.
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id]) @relation
}
"#,
    )
    .expect_err("a bare `@relation` takes no part in a relation");

    assert!(
        error
            .to_string()
            .contains("`@relation` takes an argument list"),
        "{error}",
    );
}
