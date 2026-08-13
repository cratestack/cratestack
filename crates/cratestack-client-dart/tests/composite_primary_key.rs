//! Composite `@@id([...])` primary keys must be REJECTED, not panicked on.
//!
//! `cratestack-parser` accepts `@@id([a, b])`, and `include_*_schema!`
//! has rejected it at expansion since the gap was found. This generator
//! had no equivalent guard, so `cratestack generate-dart` against
//! such a schema aborted with
//!
//!     thread 'main' panicked at src/builders_model.rs:113:
//!     validated schemas always have an id field
//!
//! — a panic instead of an error, and a claim the parser does not make.
//! Reproduced by hand on 2026-08-13 before the fix. See
//! `cratestack_core::composite_id`.

use cratestack_client_dart::{DartGeneratorConfig, DartGeneratorError, generate_package};

const COMPOSITE_PK: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

model Account {
  id Int @id
  name String
  @@allow("read", auth() != null)
}

model AccountMembership {
  accountId Int
  subject String
  account Account @relation(fields:[accountId],references:[id])
  @@id([accountId, subject])
  @@allow("read", auth() != null)
}
"#;

#[test]
fn a_composite_primary_key_is_an_error_not_a_panic() {
    let schema = cratestack_parser::parse_schema(COMPOSITE_PK)
        .expect("the parser accepts composite @@id — that is the whole premise here");
    let error = generate_package(&schema, &DartGeneratorConfig::default())
        .expect_err("a composite-PK schema must be rejected, not generated");

    let DartGeneratorError::CompositePrimaryKeyUnsupported(message) = &error else {
        panic!("expected CompositePrimaryKeyUnsupported, got: {error}");
    };
    // Names the offending model, and points at the tracking issue rather
    // than leaving the user to guess what to do.
    assert!(
        message.contains("AccountMembership") && message.contains("issues/136"),
        "message should name the model and the tracking issue:\n{message}"
    );
    // The old failure mode's wording must not come back.
    assert!(
        !message.contains("validated schemas always have an id field"),
        "that message was false — the parser accepts this schema:\n{message}"
    );
}

/// The guard must key on `@@id([...])`, not on "a model I couldn't find a
/// PK for" — an ordinary single-`@id` schema must be unaffected.
#[test]
fn a_single_scalar_primary_key_still_generates() {
    let source = COMPOSITE_PK
        .replace("  @@id([accountId, subject])\n", "")
        .replace("  subject String\n", "  subject String @id\n");
    let schema = cratestack_parser::parse_schema(&source).expect("control schema should parse");
    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("a single-@id schema must still generate");
    assert!(
        package
            .files
            .iter()
            .any(|f| f.file_name == "lib/src/apis.dart"),
        "expected a real generated package"
    );
}
