#![cfg(test)]
//! cratestack#1074: `@id` is matched exactly (not as a prefix), `@id(...)`
//! is refused, and a second `@relation` on one field is refused.

use super::parse_schema;

#[test]
fn identity_is_not_a_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  code String @identity
  name String
}
"#,
    )
    .expect_err("`@identity` must not satisfy the primary-key requirement");

    assert!(
        error
            .to_string()
            .contains("model `Account` is missing an @id field"),
        "{error}",
    );
}

#[test]
fn id_prefixed_attributes_beside_a_real_id_are_not_keys() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  code String @identity
  ref String @id_foo
}
"#,
    )
    .expect("an unknown `@id…` attribute is inert, not a second `@id`");

    let keys: Vec<&str> = schema.models[0]
        .fields
        .iter()
        .filter(|field| field.is_primary_key())
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(keys, ["id"]);
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
            error.to_string().contains("`@id` takes no arguments"),
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
fn a_bare_relation_counts_toward_the_limit() {
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
    .expect_err("a bare second `@relation` is still a second declaration");

    assert!(
        error
            .to_string()
            .contains("declares `@relation` more than once"),
        "{error}",
    );
}
