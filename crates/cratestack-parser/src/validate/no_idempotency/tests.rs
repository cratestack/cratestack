//! `@no_idempotency` attribute validation (#876).
//!
//! Lives beside the validator (`foo.rs` + `foo/tests.rs`, this crate's
//! existing shape) rather than as a top-level `tests_*.rs`: `lib.rs`'s
//! module list is 199 lines and one more `#[cfg(test)] mod` declaration
//! put it over the workspace ceiling.
//!
//! Before the validator existed, `@no_idempotency(true)` reached
//! `cratestack-cli check` and printed `schema OK`, then emitted
//! `idempotent_by_default: false` — the argument was accepted and the
//! attribute silently did the opposite of what the parenthesised value
//! said. These tests pin the rejection; the bare-form test is the control
//! that keeps the rejection from being over-broad.

use crate::parse_schema;

const BARE: &str = r#"
type Ping {
  nonce String
}

mutation procedure notify(args: Ping): Ping
  @no_idempotency
"#;

const WITH_ARGUMENT: &str = r#"
type Ping {
  nonce String
}

mutation procedure notify(args: Ping): Ping
  @no_idempotency(true)
"#;

const DUPLICATED: &str = r#"
type Ping {
  nonce String
}

mutation procedure notify(args: Ping): Ping
  @no_idempotency
  @no_idempotency
"#;

#[test]
fn bare_no_idempotency_still_parses() {
    let schema = parse_schema(BARE).expect("the bare form is the supported one");
    assert!(
        schema.procedures[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@no_idempotency"),
        "the attribute must survive validation onto the Procedure, or codegen \
         has nothing to read"
    );
}

/// Unlike `@no_rate_limit`, this attribute is NOT gated on an `extension`
/// block — deliberately, per `validate/no_idempotency.rs`'s module doc.
/// Pinned here so a future "add the gate for symmetry" change has to break
/// a test and read that rationale.
#[test]
fn no_idempotency_needs_no_extension_block() {
    let schema = parse_schema(BARE).expect("no extension block is declared above");
    assert!(schema.declared_extensions.is_empty());
}

#[test]
fn no_idempotency_rejects_arguments() {
    let error = parse_schema(WITH_ARGUMENT)
        .expect_err("@no_idempotency(true) must not be silently accepted and ignored");
    assert!(
        error.message().contains("does not take any arguments"),
        "the message must say why, got: {}",
        error.message()
    );
    assert!(
        error.message().contains("notify"),
        "the message must name the procedure, got: {}",
        error.message()
    );
}

#[test]
fn no_idempotency_rejects_duplicates() {
    let error = parse_schema(DUPLICATED).expect_err("a repeated @no_idempotency must be an error");
    assert!(
        error.message().contains("more than one @no_idempotency"),
        "got: {}",
        error.message()
    );
}
