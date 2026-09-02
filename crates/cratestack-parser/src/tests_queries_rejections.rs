//! Bad `query` **signatures**, and the message each produces
//! (cratestack#867; accepted design `docs/design/declarative-custom-query.md`
//! §2/§3): positional placeholders that do not line up with the declared
//! parameters, result types that are not a `type` block, and parameter
//! types that cannot be bound.
//!
//! All of these are *parse-time* failures — `cargo check` time for an
//! `include_server_schema!` consumer, not a running server. That is the
//! property design §2 exists to deliver, so each test asserts on the
//! message text rather than merely on `is_err()`: an error that fires for
//! the wrong reason is not the check anyone was promised.
//!
//! Attribute- and name-level rejections live in the sibling
//! [`tests_queries_attributes`](crate::tests_queries_attributes). The two
//! macro-level ones (`include_embedded_schema!` and `db = None`) live in
//! `cratestack-macros`' `tests/ui_query.rs`, where trybuild can pin the
//! real rustc diagnostic.

use crate::tests_queries_support::{error_for, with_query};

#[test]
fn rejects_a_placeholder_past_the_declared_parameter_count() {
    let message = error_for(&with_query(
        r#"query totals(userId: String, cutoff: DateTime): Totals
  @@sql("SELECT 1 AS total WHERE a = $1 AND b = $3")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("references parameter `$3`"), "{message}");
    assert!(
        message.contains("only 2 parameter(s) are declared (`userId`, `cutoff`)"),
        "{message}"
    );
}

#[test]
fn rejects_a_zero_placeholder() {
    // Postgres parameters are 1-based; `$0` would fail at bind time with
    // an error pointing inside generated code.
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $0")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("references parameter `$0`"), "{message}");
}

#[test]
fn rejects_a_declared_parameter_that_the_body_never_references() {
    // The typo the epic worried about: `$3` written for `$2` leaves
    // `cutoff` silently unused. Checking only the other direction would
    // catch the `$3` but not tell the author which parameter went dead.
    let message = error_for(&with_query(
        r#"query totals(userId: String, cutoff: DateTime): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("declares parameter `cutoff` (`$2`) but it is never referenced"),
        "{message}"
    );
}

#[test]
fn rejects_an_unknown_result_type() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Nope
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("unknown result type"), "{message}");
    assert!(message.contains("no `type Nope` is declared"), "{message}");
}

#[test]
fn rejects_a_model_as_the_result_type() {
    // Design §6: a `query`'s raw SQL gets no soft-delete or row-policy
    // filtering, so handing back a `Model` would look like a filtered
    // model read when it is nothing of the kind.
    let message = error_for(
        r#"
model Event {
  id Int @id
}

query totals(userId: String): Event
  @@sql("SELECT 1 AS id WHERE a = $1")
  @allow(auth() != null)
"#,
    );
    assert!(message.contains("is not a `type` declaration"), "{message}");
}

#[test]
fn rejects_an_unbindable_parameter_type() {
    let message = error_for(&with_query(
        r#"query totals(page: PageInput): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("cannot be bound as a SQL parameter"),
        "{message}"
    );
}

#[test]
fn rejects_a_list_parameter() {
    let message = error_for(&with_query(
        r#"query totals(ids: String[]): Totals
  @@sql("SELECT 1 AS total WHERE a = ANY($1)")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("must be a required scalar"), "{message}");
}

#[test]
fn rejects_a_header_with_no_result_type() {
    let message = error_for(&with_query(
        r#"query totals(userId: String)
  @@sql("SELECT 1 AS total WHERE a = $1")"#,
    ));
    assert!(message.contains("must include a result type"), "{message}");
}

/// The end-to-end half of the scanner's `E'…'` fix (cratestack#870 review
/// round 2). Measured before the fix: this schema **compiled**, because
/// the escape-string's `\'` was read as closing the literal and the `'`
/// after it as opening a new one, swallowing the `$5` that should have
/// been rejected.
///
/// Worth having at this level and not only in `cratestack-core`'s scanner
/// unit tests: the scanner returning the right set is a means, whereas
/// "the schema does not build" is the guarantee an author relies on.
#[test]
fn rejects_an_out_of_range_placeholder_hidden_after_an_escape_string() {
    let declaration = concat!(
        "query totals(userId: String): Totals\n",
        "  @@sql(\"\"\"\n",
        "    SELECT 1 AS total FROM t WHERE a = $1 AND note = E'\\'' AND x = $5\n",
        "  \"\"\")\n",
        "  @allow(auth() != null)",
    );
    let message = error_for(&with_query(declaration));
    assert!(message.contains("references parameter `$5`"), "{message}");
    assert!(
        message.contains("only 1 parameter(s) are declared"),
        "{message}"
    );
}
