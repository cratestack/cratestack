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

/// cratestack#536: two field-level `@id` attributes on one model is a
/// multi-column primary key by another spelling — `@@id([a, b])` is
/// hard-rejected at macro expansion citing #136
/// (`reject_composite_primary_keys`), but nothing stopped this
/// equivalent form from reaching `cratestack-migrate`, which marks
/// every `@id`-tagged column `primary_key = true` and happily emits a
/// real multi-column `PRIMARY KEY`. The front door was locked, the
/// back door was open — this closes it at parse time so both
/// spellings are rejected identically.
///
/// (Decisive-test history: before this fix landed, this same
/// assertion was `parse_schema(...).expect("schema currently parses
/// and validates cleanly — this is the bug")`, and it passed —
/// proving the gap existed before it was closed.)
#[test]
fn rejects_two_field_level_id_attributes() {
    let error = parse_schema(
        r#"
model Thing {
  a String @id
  b String @id
}
"#,
    )
    .expect_err("schema should fail validation");

    let message = error.to_string();
    assert!(
        message.contains("more than one field-level `@id`"),
        "unexpected message: {message}"
    );
    assert!(
        message.contains("cratestack/cratestack/issues/136"),
        "expected the error to point at the same #136 reasoning as `@@id([...])`'s rejection: {message}"
    );
}

/// cratestack#327: `datasource { provider = "none" }` is a third accepted
/// provider value, for no-database procedures-only schemas.
#[test]
fn accepts_datasource_provider_none_with_zero_models() {
    let schema = parse_schema(
        r#"
datasource db {
  provider = "none"
}

type Ping {
  message String
}

procedure ping(): Ping
"#,
    )
    .expect("datasource none with zero models should validate cleanly");

    assert!(schema.models.is_empty());
    assert_eq!(schema.procedures.len(), 1);
}

/// `datasource { provider = "none" }` with zero procedures too — the story's
/// acceptance criteria explicitly calls out "or even zero-procedure".
#[test]
fn accepts_datasource_provider_none_with_zero_models_and_zero_procedures() {
    parse_schema(
        r#"
datasource db {
  provider = "none"
}
"#,
    )
    .expect("datasource none with nothing else should validate cleanly");
}

/// cratestack#327: a `model` block under `datasource { provider = "none" }`
/// must be rejected with an error naming the offending model, not a generic
/// failure.
#[test]
fn rejects_model_block_under_datasource_provider_none() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "none"
}

model User {
  id Int @id
}
"#,
    )
    .expect_err("model block under datasource none should fail validation");

    let message = error.to_string();
    assert!(
        message.contains("User"),
        "error should name the offending model: {message}"
    );
    assert!(
        message.contains("provider = \"none\""),
        "error should explain why: {message}"
    );
}

/// Regression: multiple models under `datasource none` still name a model
/// (the first one) rather than falling back to a generic message.
#[test]
fn rejects_first_model_block_under_datasource_provider_none() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "none"
}

model Account {
  id Int @id
}

model User {
  id Int @id
}
"#,
    )
    .expect_err("model blocks under datasource none should fail validation");

    assert!(error.to_string().contains("Account"));
}

/// Existing `"postgresql"`/`"sqlite"` behavior must be completely
/// unaffected by the new `"none"` provider value.
#[test]
fn postgresql_and_sqlite_providers_still_allow_models() {
    for provider in ["postgresql", "sqlite"] {
        let schema = parse_schema(&format!(
            r#"
datasource db {{
  provider = "{provider}"
}}

model User {{
  id Int @id
}}
"#,
        ))
        .unwrap_or_else(|error| {
            panic!("provider `{provider}` with a model should validate: {error}")
        });

        assert_eq!(schema.models.len(), 1);
    }
}

/// Existing invalid-provider rejection is unchanged by adding `"none"`.
#[test]
fn rejects_unsupported_datasource_provider() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "mysql"
}
"#,
    )
    .expect_err("unsupported provider should fail validation");

    assert!(
        error
            .to_string()
            .contains("unsupported datasource provider")
    );
}
