#![cfg(test)]
//! `cratestack check` / `print-ir` accept a schema declaring a `query`
//! block (cratestack#867).
//!
//! Neither handler needed a code change for this — `check` delegates to
//! `cratestack_parser::parse_schema_file` and `print-ir` `Debug`-prints
//! the whole `Schema`, so both pick up `Schema::queries` for free. That is
//! exactly why these tests exist: "works by construction" is the kind of
//! claim that stops being true the moment someone adds a per-construct
//! `match` to either handler, and nothing else in the suite would notice.
//!
//! `print-ir` writes to stdout, which a unit test cannot capture without
//! process-level plumbing, so the assertion here is on the same parsed IR
//! the handler formats — the value, not the formatting.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::{handle_check, handle_print_ir};
use crate::cli_types::OutputFormat;

const SCHEMA_WITH_QUERY: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

type Totals {
  total Int
  thisMonth Int
}

query loyaltyFeeSummary(userId: String, cutoff: DateTime): Totals
  @@sql("""
    SELECT
      COALESCE(SUM(discount), 0)::bigint AS "total",
      COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS "thisMonth"
    FROM loyalty_fee_events
    WHERE user_id = $1
  """)
  @allow(auth() != null)
"#;

const SCHEMA_WITH_BAD_PLACEHOLDER: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

type Totals {
  total Int
}

query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $2")
  @allow(auth() != null)
"#;

fn write_schema(dir: &TempDir, source: &str) -> PathBuf {
    let path = dir.path().join("schema.cstack");
    fs::write(&path, source).expect("write schema");
    path
}

#[test]
fn check_accepts_a_schema_declaring_a_query() {
    let dir = TempDir::new().expect("temp dir");
    let path = write_schema(&dir, SCHEMA_WITH_QUERY);

    handle_check(path, OutputFormat::Human).expect("check should accept a query block");
}

#[test]
fn check_rejects_an_out_of_range_placeholder() {
    // Guards the test above against being vacuous: `check` has to actually
    // be running the query validators, not merely tolerating the syntax.
    let dir = TempDir::new().expect("temp dir");
    let path = write_schema(&dir, SCHEMA_WITH_BAD_PLACEHOLDER);

    let error = handle_check(path, OutputFormat::Human)
        .expect_err("check should reject a `$2` with one declared parameter");
    let message = error.to_string();
    assert!(message.contains("references parameter `$2`"), "{message}");
}

#[test]
fn print_ir_accepts_a_schema_declaring_a_query() {
    let dir = TempDir::new().expect("temp dir");
    let path = write_schema(&dir, SCHEMA_WITH_QUERY);

    handle_print_ir(path.clone()).expect("print-ir should accept a query block");

    // The value `print-ir` formats — asserted directly, since the handler
    // itself writes to stdout.
    let schema = cratestack_parser::parse_schema_file(&path).expect("schema should parse");
    let query = schema
        .queries
        .first()
        .expect("the query should be present in the IR print-ir renders");
    assert_eq!(query.name, "loyaltyFeeSummary");
    assert_eq!(query.result_type.name, "Totals");
    assert!(
        query
            .sql()
            .expect("query should carry a SQL body")
            .contains("FILTER (WHERE created_at >= $2)"),
    );
}
