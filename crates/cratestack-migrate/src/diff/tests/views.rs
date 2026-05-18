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
fn drop_view_lands_before_drop_column() {
    // Regression for the Codex #1 P2 on #89: the previous ordering
    // appended view drops after `drop_columns`, so a migration that
    // dropped a column the view referenced would have the column
    // drop rejected by Postgres (view still alive, depends on
    // column). View drops must land BEFORE column drops.
    let prev = schema_with_view(ACTIVE_CUSTOMER_VIEW);
    // Remove the `email` field that the view body reads.
    let next = schema(&with_models(
        r#"
model Customer {
  id Int @id
}
"#,
    ));
    let ops = diff(&prev, &next);

    let drop_view_idx = ops
        .iter()
        .position(|op| matches!(op, Op::DropView(_)))
        .expect("DropView op");
    let drop_column_idx = ops
        .iter()
        .position(|op| matches!(op, Op::DropColumn(_)))
        .expect("DropColumn op for `email`");
    assert!(
        drop_view_idx < drop_column_idx,
        "DropView must land before DropColumn; got DropView@{drop_view_idx} \
         and DropColumn@{drop_column_idx}"
    );
}

#[test]
fn body_change_emits_drop_then_create_in_order() {
    // Regression for Codex #2 P2 on #89: body changes used to emit a
    // single `ReplaceView` op flushed at the end of the op vec, which
    // wouldn't run before column drops in the same migration. We
    // now model body changes as Drop + Create with the drop in the
    // pre-column-drops bucket and the create in the post-column-adds
    // bucket.
    let prev = schema_with_view(ACTIVE_CUSTOMER_VIEW);
    let next = schema_with_view(ACTIVE_CUSTOMER_VIEW_NEW_BODY);
    let ops = diff(&prev, &next);

    let drop_idx = ops
        .iter()
        .position(|op| matches!(op, Op::DropView(_)))
        .expect("DropView op for body change");
    let create_idx = ops
        .iter()
        .position(|op| matches!(op, Op::CreateView(_)))
        .expect("CreateView op for body change");
    assert!(
        drop_idx < create_idx,
        "DropView must precede CreateView in body-change migration"
    );
    // ReplaceView is no longer emitted by the diff engine.
    assert!(
        !ops.iter().any(|op| matches!(op, Op::ReplaceView(_))),
        "body changes should emit Drop + Create, not ReplaceView"
    );
}

#[test]
fn materialized_view_skipped_on_sqlite_projection() {
    // Regression for Codex #3 P2 on #89: a `@@materialized` view
    // with shared `@@sql(...)` pointed at a SQLite datasource used
    // to slip through the SQLite emitter's `unreachable!`. The diff
    // engine now filters materialized views out of the SQLite
    // projection so the SQL emitter never sees them.
    let sqlite_models = r#"
model Customer {
  id Int @id
  email String
}

view CustomerSnapshot from Customer {
  id Int @id @from(Customer.id)
  email String @from(Customer.email)
  @@sql("SELECT id, email FROM customers")
  @@materialized
}
"#;
    let prev_with_sqlite_datasource = r#"
datasource db {
  provider = "sqlite"
}
"#
    .to_owned();
    let prev = schema(&prev_with_sqlite_datasource);
    let next = schema(&format!(
        "{prev_with_sqlite_datasource}{sqlite_models}"
    ));
    let ops = diff(&prev, &next);

    // No view ops at all — the materialized view was filtered out
    // because the datasource is SQLite.
    assert!(
        !ops.iter().any(|op| matches!(
            op,
            Op::CreateView(_)
                | Op::CreateMaterializedView(_)
                | Op::DropView(_)
                | Op::DropMaterializedView(_)
        )),
        "materialized view should not produce ops on a SQLite datasource"
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
