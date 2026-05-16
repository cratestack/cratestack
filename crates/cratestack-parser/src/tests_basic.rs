#![cfg(test)]

use super::parse_schema;

#[test]
fn parses_and_validates_initial_schema_subset() {
    let schema = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth UserAuth {
  id Int
  role String
}

model User {
  id Int @id
  email String @unique
  role String

  @@allow("read", auth() != null)
}

type PublishPostInput {
  postId Int
}

mutation procedure publishPost(args: PublishPostInput): User
  @allow(auth().role == "admin")
"#,
    )
    .expect("schema should parse");

    assert_eq!(schema.models.len(), 1);
    assert_eq!(schema.types.len(), 1);
    assert_eq!(schema.procedures.len(), 1);
}

#[test]
fn rejects_models_without_primary_keys() {
    let error = parse_schema(
        r#"
model User {
  email String
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("missing an @id field"));
}
