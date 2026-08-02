#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_ondelete_and_onupdate_on_the_owning_side() {
    let schema = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id],onDelete: Cascade,onUpdate: Restrict)
}
"#,
    )
    .expect("schema should parse");
    assert_eq!(schema.models.len(), 2);
}

#[test]
fn rejects_unknown_action_vocabulary() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id],onDelete: Nuke)
}
"#,
    )
    .expect_err("schema should fail validation");
    assert!(error.to_string().contains("invalid relation action `Nuke`"));
}

#[test]
fn rejects_set_null_on_a_required_local_field() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id],onDelete: SetNull)
}
"#,
    )
    .expect_err("schema should fail validation");
    assert!(error.to_string().contains("onDelete: SetNull"));
    assert!(error.to_string().contains("not optional"));
}

#[test]
fn accepts_set_null_on_an_optional_local_field() {
    let schema = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int?
  author User? @relation(fields:[authorId],references:[id],onDelete: SetNull)
}
"#,
    )
    .expect("schema should parse");
    assert_eq!(schema.models.len(), 2);
}

#[test]
fn rejects_set_default_without_a_default_value() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id],onUpdate: SetDefault)
}
"#,
    )
    .expect_err("schema should fail validation");
    assert!(error.to_string().contains("onUpdate: SetDefault"));
    assert!(error.to_string().contains("has no @default"));
}

#[test]
fn accepts_set_default_with_a_default_value() {
    let schema = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int @default(0)
  author User @relation(fields:[authorId],references:[id],onUpdate: SetDefault)
}
"#,
    )
    .expect("schema should parse");
    assert_eq!(schema.models.len(), 2);
}

#[test]
fn rejects_ondelete_declared_on_the_has_many_side() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
  posts Post[] @relation(fields:[id],references:[authorId],onDelete: Cascade)
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id])
}
"#,
    )
    .expect_err("schema should fail validation");
    assert!(error.to_string().contains("has-many side"));
}
