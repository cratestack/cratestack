//! Bad `query` **attributes and names** (cratestack#867): unrecognised
//! attributes, colliding names, and a query in a schema that configures no
//! database.
//!
//! The unrecognised-attribute case is the one worth not dropping: without
//! it, a misspelled `@alow` would parse silently and leave the query
//! deny-by-default with nothing to explain why.
//!
//! Everything about the *SQL body itself* — missing, duplicated,
//! malformed, or needing escapes — lives in
//! [`tests_queries_sql_body`](crate::tests_queries_sql_body), which is
//! also where the `view` half of those shared rules is checked.
//! Signature-level rejections live in
//! [`tests_queries_rejections`](crate::tests_queries_rejections).

use crate::tests_queries_support::{error_for, with_query};

#[test]
fn rejects_an_unsupported_attribute() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)
  @stream"#,
    ));
    assert!(
        message.contains("unsupported attribute `@stream`"),
        "{message}"
    );
}

#[test]
fn rejects_a_duplicate_query_name() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)

query totals(userId: String): Totals
  @@sql("SELECT 2 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("duplicate query name `totals`"),
        "{message}"
    );
}

#[test]
fn rejects_two_queries_that_would_generate_the_same_module() {
    let message = error_for(&with_query(
        r#"query monthTotals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)

query month_totals(userId: String): Totals
  @@sql("SELECT 2 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("both generate the module `month_totals`"),
        "{message}"
    );
}

#[test]
fn rejects_a_query_when_the_schema_configures_no_database() {
    let message = error_for(
        r#"
datasource db {
  provider = "none"
}

type Totals {
  total Int
}

query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)
"#,
    );
    assert!(
        message.contains("configures no database for a `query` to run against"),
        "{message}"
    );
}
