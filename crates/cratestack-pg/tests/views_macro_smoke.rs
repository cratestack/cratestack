//! Compile-only smoke test for the `view` macro emission path.
//!
//! Exercises [`include_server_schema!`] with a fixture that declares
//! a view, then references the macro-emitted types from the generated
//! `cratestack_schema::views::Views` accessor module. If any of the
//! generators (view struct / ViewDescriptor / FromRow / accessor) is
//! malformed the test fails to compile.
//!
//! No database connection required — every assertion below is a
//! type-level reference resolved at compile time.

cratestack::include_server_schema!("tests/fixtures/views_smoke.cstack", db = Postgres);

#[test]
fn view_macro_emits_struct_descriptor_and_accessor() {
    // The view struct exists with the declared scalar fields.
    let _: cratestack_schema::ActiveCustomer = cratestack_schema::ActiveCustomer {
        id: 1,
        email: "x@example.com".to_owned(),
        orderCount: 0,
    };

    // The descriptor const exists and reports the right metadata.
    let descriptor = &cratestack_schema::models::ACTIVE_CUSTOMER_VIEW;
    assert_eq!(descriptor.view_name, "active_customer");
    assert_eq!(descriptor.primary_key, "id");
    assert_eq!(descriptor.columns.len(), 3);
    assert_eq!(descriptor.source_tables, &["customers", "orders"]);
    assert!(!descriptor.is_materialized);

    // Schema summary surfaces the view name.
    let summary = cratestack_schema::schema_summary();
    assert!(summary.views.contains(&"ActiveCustomer"));
}
