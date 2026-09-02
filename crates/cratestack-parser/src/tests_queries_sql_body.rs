//! The `@@sql` body itself: missing, duplicated, or written in a form that
//! does not parse as a quoted string (cratestack#867, and findings 2 and 6
//! of cratestack#870's review).
//!
//! Two of these are shared rules rather than query-specific ones — `view`
//! reads its `@@server_sql` through the same extractor — so the `view`
//! half is checked here too, next to the `query` half it has to agree
//! with.
//!
//! The malformed cases matter more than they look. Each one used to
//! *count* as a body while yielding none, so the query compiled with
//! `SQL = ""` and every `$N` check skipped: a schema that looked fine, and
//! could never work.

use crate::tests_queries_support::{error_for, with_query};

#[test]
fn rejects_a_query_with_no_sql_body() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("has no SQL body"), "{message}");
}

#[test]
fn rejects_the_per_backend_sql_split_a_view_allows() {
    // `query` is Postgres-only (design §4). Accepting `@@embedded_sql`'s
    // spelling would advertise a backend that does not exist.
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@embedded_sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("Postgres-only"), "{message}");
    assert!(message.contains("@@embedded_sql"), "{message}");
}

/// A `@@sql` attribute whose argument is not a quoted string used to
/// *count* as a body while `Query::sql()` returned `None`, so the query
/// compiled with `SQL = ""` and every `$N` check skipped
/// (cratestack#870 review finding 2). Three spellings reach that state,
/// and all three must be rejected — a schema that looks fine and can
/// never work is worse than one that fails to build.
#[test]
fn rejects_an_unquoted_sql_argument() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql(SELECT 1 AS total WHERE a = $1)
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("argument is not a quoted string"),
        "{message}"
    );
}

#[test]
fn rejects_a_bare_sql_attribute_with_no_parentheses() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("argument is not a quoted string"),
        "{message}"
    );
}

#[test]
fn rejects_a_second_attribute_sharing_the_sql_line() {
    // Everything up to the LAST `)` on the line is read as the SQL
    // argument, so this parses as one attribute with a body that does not
    // end in a quote. The message says so rather than leaving the author
    // to work out why an apparently well-formed line was refused.
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1") @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("argument is not a quoted string"),
        "{message}"
    );
    assert!(message.contains("on its own line"), "{message}");
}

/// The same extractor backs `view`'s `@@server_sql`, where the silent
/// failure mode is different but no better: the view reads as
/// embedded-only and the server composer skips it without a word.

#[test]
fn rejects_an_unquoted_sql_argument_on_a_view_too() {
    let message = error_for(
        r#"
model Customer {
  id Int @id
  email String
}

view CustomerSummary from Customer {
  id Int @id
  email String

  @@server_sql(SELECT id, email FROM customers)
  @@allow("read", auth() != null)
}
"#,
    );
    assert!(
        message.contains("argument is not a quoted string"),
        "{message}"
    );
}

/// A single-line body may need `\"` to alias a result column, and those
/// escapes must reach Postgres as plain quotes — passing the backslashes
/// through produced a syntax error at first execution
/// (cratestack#867 review finding 6).

#[test]
fn unescapes_quotes_in_a_single_line_sql_body() {
    let declaration = concat!(
        "query totals(userId: String): Totals\n",
        "  @@sql(\"SELECT 1 AS \\\"total\\\" WHERE a = $1\")\n",
        "  @allow(auth() != null)",
    );
    let schema = crate::parse_schema(&with_query(declaration))
        .expect("a single-line body with escaped quotes should parse");

    assert_eq!(
        schema.queries[0].sql().as_deref(),
        Some("SELECT 1 AS \"total\" WHERE a = $1"),
    );
}
