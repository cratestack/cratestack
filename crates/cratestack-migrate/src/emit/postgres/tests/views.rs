//! Postgres emitter tests for view DDL (ADR-0003).

use crate::diff::diff;
use crate::emit::postgres::emit;

use super::{schema, with_models};

const SCHEMA_WITHOUT_VIEW: &str = r#"
model Customer {
  id Int @id
  email String
}
"#;

const SCHEMA_WITH_VIEW: &str = r#"
model Customer {
  id Int @id
  email String
}

view ActiveCustomer from Customer {
  id Int @id @from(Customer.id)
  email String @from(Customer.email)
  @@server_sql("SELECT id, email FROM customers")
}
"#;

const SCHEMA_WITH_MATERIALIZED_VIEW: &str = r#"
model Customer {
  id Int @id
  email String
}

view CustomerSnapshot from Customer {
  id Int @id @from(Customer.id)
  email String @from(Customer.email)
  @@server_sql("SELECT id, email FROM customers")
  @@materialized
}
"#;

#[test]
fn emits_create_view() {
    let prev = schema(&with_models(SCHEMA_WITHOUT_VIEW));
    let next = schema(&with_models(SCHEMA_WITH_VIEW));
    let ops = diff(&prev, &next);
    let migration = emit(&ops);
    let up = &migration.up;

    assert!(up.contains("CREATE VIEW active_customers AS"));
    assert!(up.contains("SELECT id, email FROM customers"));

    // The down body drops the view (`CreateView` is `Safe`, so the
    // down is auto-reversed).
    assert!(migration.down.contains("DROP VIEW active_customers;"));
    assert!(!migration.has_lossy, "CreateView is non-destructive");
}

#[test]
fn emits_create_materialized_view_with_unique_index() {
    let prev = schema(&with_models(SCHEMA_WITHOUT_VIEW));
    let next = schema(&with_models(SCHEMA_WITH_MATERIALIZED_VIEW));
    let ops = diff(&prev, &next);
    let up = emit(&ops).up;

    assert!(up.contains("CREATE MATERIALIZED VIEW customer_snapshots AS"));
    // Unique index is the precondition for
    // `REFRESH MATERIALIZED VIEW CONCURRENTLY`.
    assert!(up.contains("CREATE UNIQUE INDEX customer_snapshots_pkey ON customer_snapshots (id);"));
}

#[test]
fn emits_create_or_replace_view_on_body_change() {
    let prev = schema(&with_models(SCHEMA_WITH_VIEW));
    let next_sql = SCHEMA_WITH_VIEW.replace(
        "SELECT id, email FROM customers",
        "SELECT id, email FROM customers WHERE email IS NOT NULL",
    );
    let next = schema(&with_models(&next_sql));
    let ops = diff(&prev, &next);
    let up = emit(&ops).up;

    assert!(up.contains("CREATE OR REPLACE VIEW active_customers AS"));
    assert!(up.contains("WHERE email IS NOT NULL"));
}
