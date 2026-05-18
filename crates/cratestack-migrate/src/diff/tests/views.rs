//! Diff tests for `view` blocks (ADR-0003).

use crate::diff::diff;
use crate::ir::Op;

use super::{schema, with_models};

const SIMPLE_MODELS: &str = r#"
model Customer {
  id Int @id
  email String
}
"#;

fn schema_without_views() -> cratestack_core::Schema {
    schema(&with_models(SIMPLE_MODELS))
}

fn schema_with_view(view_block: &str) -> cratestack_core::Schema {
    schema(&format!("{}{view_block}", with_models(SIMPLE_MODELS)))
}

const ACTIVE_CUSTOMER_VIEW: &str = r#"
view ActiveCustomer from Customer {
  id Int @id @from(Customer.id)
  email String @from(Customer.email)
  @@server_sql("SELECT id, email FROM customers")
}
"#;

const ACTIVE_CUSTOMER_VIEW_NEW_BODY: &str = r#"
view ActiveCustomer from Customer {
  id Int @id @from(Customer.id)
  email String @from(Customer.email)
  @@server_sql("SELECT id, email FROM customers WHERE email IS NOT NULL")
}
"#;

#[test]
fn create_view_emits_create_view_op() {
    let prev = schema_without_views();
    let next = schema_with_view(ACTIVE_CUSTOMER_VIEW);
    let ops = diff(&prev, &next);

    let create_view = ops
        .iter()
        .find_map(|op| match op {
            Op::CreateView(view) => Some(view),
            _ => None,
        })
        .expect("CreateView op should be emitted");
    assert_eq!(create_view.name, "active_customers");
    assert!(create_view.sql.contains("SELECT id, email FROM customers"));
    assert_eq!(create_view.source_tables, &["customers"]);
}

#[test]
fn drop_view_emits_drop_view_op() {
    let prev = schema_with_view(ACTIVE_CUSTOMER_VIEW);
    let next = schema_without_views();
    let ops = diff(&prev, &next);

    assert!(ops.iter().any(|op| matches!(op, Op::DropView(_))));
}

#[test]
fn body_change_emits_replace_view_op() {
    let prev = schema_with_view(ACTIVE_CUSTOMER_VIEW);
    let next = schema_with_view(ACTIVE_CUSTOMER_VIEW_NEW_BODY);
    let ops = diff(&prev, &next);

    let replace = ops
        .iter()
        .find_map(|op| match op {
            Op::ReplaceView(view) => Some(view),
            _ => None,
        })
        .expect("ReplaceView op should be emitted");
    assert!(replace.sql.contains("WHERE email IS NOT NULL"));
}

#[test]
fn create_view_lands_after_create_table() {
    // Add a brand-new model + view in the same diff. The view's
    // CreateView must appear in the op vec *after* the CreateTable
    // for its source model — otherwise the migration runs DDL in an
    // order Postgres rejects.
    let prev = schema(&with_models(""));
    let next = schema_with_view(ACTIVE_CUSTOMER_VIEW);
    let ops = diff(&prev, &next);

    let create_table_idx = ops
        .iter()
        .position(|op| matches!(op, Op::CreateTable(_)))
        .expect("CreateTable op for Customer");
    let create_view_idx = ops
        .iter()
        .position(|op| matches!(op, Op::CreateView(_)))
        .expect("CreateView op for ActiveCustomer");
    assert!(
        create_table_idx < create_view_idx,
        "CreateTable must land before CreateView; got CreateTable@{create_table_idx} \
         and CreateView@{create_view_idx}"
    );
}

#[test]
fn drop_view_lands_before_drop_table() {
    // Drop both the view and its source model in one diff. The
    // DropView must come before the DropTable — Postgres refuses to
    // drop a table that has a dependent view.
    let prev = schema_with_view(ACTIVE_CUSTOMER_VIEW);
    let next = schema(&with_models(""));
    let ops = diff(&prev, &next);

    let drop_view_idx = ops
        .iter()
        .position(|op| matches!(op, Op::DropView(_)))
        .expect("DropView op");
    let drop_table_idx = ops
        .iter()
        .position(|op| matches!(op, Op::DropTable(_)))
        .expect("DropTable op");
    assert!(
        drop_view_idx < drop_table_idx,
        "DropView must land before DropTable; got DropView@{drop_view_idx} \
         and DropTable@{drop_table_idx}"
    );
}
