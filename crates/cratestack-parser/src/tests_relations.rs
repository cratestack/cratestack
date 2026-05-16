#![cfg(test)]

use super::{parse_relation_attribute, parse_schema};

#[test]
fn rejects_relation_fields_without_explicit_relation_metadata() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("must declare @relation"));
}

#[test]
fn rejects_relations_with_unknown_local_fields() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[ownerId],references:[id])
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("unknown local field `ownerId`"));
}

#[test]
fn rejects_relations_with_unknown_target_fields() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[userId])
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("unknown target field `userId`"));
}

#[test]
fn rejects_relations_with_incompatible_scalar_reference_types() {
    let error = parse_schema(
        r#"
model User {
  id String @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id])
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(
        error
            .to_string()
            .contains("links incompatible scalar types")
    );
}

#[test]
fn preserves_precise_name_spans_for_relations_and_type_references() {
    let source = r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id])
}
"#;
    let schema = parse_schema(source).expect("schema should parse");
    let post = &schema.models[1];
    let author = &post.fields[2];
    let relation =
        parse_relation_attribute(&author.attributes[0].raw).expect("relation should parse");

    assert_eq!(&source[post.name_span.start..post.name_span.end], "Post");
    assert_eq!(
        &source[author.name_span.start..author.name_span.end],
        "author"
    );
    assert_eq!(
        &source[author.ty.name_span.start..author.ty.name_span.end],
        "User"
    );
    assert_eq!(relation.fields, vec!["authorId".to_owned()]);
    assert_eq!(relation.references, vec!["id".to_owned()]);
}

#[test]
fn tracks_field_type_span_from_token_position_not_first_substring_match() {
    let source = r#"
model Group {
  id Int @id
}

model User {
  id Int @id
  groupId Int
  GroupLabel Group @relation(fields:[groupId],references:[id])
}
"#;
    let schema = parse_schema(source).expect("schema should parse");
    let field = &schema.models[1].fields[2];

    assert_eq!(
        &source[field.name_span.start..field.name_span.end],
        "GroupLabel"
    );
    assert_eq!(
        &source[field.ty.name_span.start..field.ty.name_span.end],
        "Group"
    );
}
