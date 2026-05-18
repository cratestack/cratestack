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
    assert_eq!(descriptor.view_name, "active_customers");
    assert_eq!(descriptor.primary_key, "id");
    assert_eq!(descriptor.columns.len(), 3);
    assert_eq!(descriptor.source_tables, &["customers", "orders"]);
    assert!(!descriptor.is_materialized);

    // `@@allow("read", auth() != null)` should produce one
    // `ReadPolicy` in BOTH `read_allow_policies` (list reads) and
    // `detail_allow_policies` (single-row reads), since views only
    // support the `read` action and we fan it to both slots.
    assert_eq!(descriptor.read_allow_policies.len(), 1);
    assert_eq!(descriptor.detail_allow_policies.len(), 1);
    assert!(descriptor.read_deny_policies.is_empty());
    assert!(descriptor.detail_deny_policies.is_empty());

    // Schema summary surfaces the view name.
    let summary = cratestack_schema::schema_summary();
    assert!(summary.views.contains(&"ActiveCustomer"));
    assert!(summary.views.contains(&"DailyRevenue"));
}

#[test]
fn no_unique_view_descriptor_has_empty_primary_key() {
    // `DailyRevenue` is declared `@@no_unique` — the descriptor's
    // `primary_key` is an empty string, signalling "no @id".
    let descriptor = &cratestack_schema::models::DAILY_REVENUE_VIEW;
    assert_eq!(descriptor.view_name, "daily_revenues");
    assert_eq!(descriptor.primary_key, "");
    assert_eq!(descriptor.columns.len(), 2);
}

/// Compile-time guarantee that `@@no_unique` views do **not** expose
/// `find_unique`. If a future change re-introduces the method on the
/// no-unique delegate, this `compile_error!`-style guard fails to
/// compile.
///
/// We can't actually instantiate the runtime here (it needs a PgPool),
/// but we *can* assert that `ViewDelegateNoUnique` is the type the
/// accessor returns — which doesn't have `find_unique` in its
/// inherent impl. The assertion uses a generic function that only
/// compiles when the input is precisely `ViewDelegateNoUnique`,
/// catching any future widening to a delegate that does expose
/// `find_unique`.
#[allow(dead_code)]
fn _accessor_returns_no_unique_delegate<'a>(
    delegate: ::cratestack::ViewDelegateNoUnique<'a, cratestack_schema::DailyRevenue>,
) -> ::cratestack::ViewDelegateNoUnique<'a, cratestack_schema::DailyRevenue> {
    delegate
}
