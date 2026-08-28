#![cfg(test)]
//! A bespoke `procedure` and a model's generated CRUD handler can land on
//! the same Rust ident in the axum module `include_server_schema!` emits
//! (`model Order` + `procedure getOrder` → `handle_get_order` twice). The
//! parser used to say `schema OK` and leave rustc to report a bare
//! `error[E0428]` naming neither declaration — see
//! `crates/cratestack-parser/src/validate/procedure_handler_collisions.rs`.

use crate::parse_schema;

fn collision_error(source: &str) -> String {
    parse_schema(source)
        .expect_err("a procedure colliding with a generated CRUD handler must be rejected")
        .to_string()
}

const ORDER_MODEL: &str = r#"
model Order {
  id String @id
  total Int
}
"#;

fn schema_with_procedure(signature: &str) -> String {
    format!("{ORDER_MODEL}\nprocedure {signature}\n")
}

#[test]
fn rejects_get_procedure_colliding_with_the_model_get_handler() {
    let message = collision_error(&schema_with_procedure("getOrder(orderId: String): Order"));
    assert!(message.contains("getOrder"), "error: {message}");
    assert!(message.contains("Order"), "error: {message}");
    assert!(message.contains("handle_get_order"), "error: {message}");
    assert!(message.contains("rename the procedure"), "error: {message}");
}

#[test]
fn rejects_update_and_delete_procedures_on_the_singular_stem() {
    for signature in [
        "updateOrder(orderId: String): Order",
        "deleteOrder(orderId: String): Order",
    ] {
        let message = collision_error(&schema_with_procedure(signature));
        assert!(message.contains("Order"), "error: {message}");
    }
}

#[test]
fn rejects_list_and_create_procedures_on_the_pluralized_stem() {
    // `list`/`create` hang off the collection route, so their handler
    // stems are pluralized — `handle_list_orders`, not `handle_list_order`.
    for (signature, ident) in [
        ("listOrders(limit: Int): Order", "handle_list_orders"),
        ("createOrders(total: Int): Order", "handle_create_orders"),
    ] {
        let message = collision_error(&schema_with_procedure(signature));
        assert!(message.contains(ident), "error: {message}");
    }
}

#[test]
fn rejects_a_snake_case_spelled_procedure_identically() {
    // Detection runs on the `to_snake_case`-normalized form, so the
    // collision is caught however the procedure is spelled in the schema.
    let message = collision_error(&schema_with_procedure("get_order(orderId: String): Order"));
    assert!(message.contains("handle_get_order"), "error: {message}");
}

#[test]
fn rejects_a_procedure_colliding_with_the_dispatch_twin() {
    // Every handler has a `_dispatch` twin used by the RPC transport, so
    // `getOrderDispatch` collides even though `handle_get_order_dispatch`
    // is never spelled out in the schema.
    let message = collision_error(&schema_with_procedure(
        "getOrderDispatch(orderId: String): Order",
    ));
    assert!(
        message.contains("handle_get_order_dispatch"),
        "error: {message}"
    );
}

#[test]
fn rejects_the_collision_on_a_multi_word_model_name() {
    let source = r#"
model SubOrder {
  id String @id
  total Int
}

procedure getSubOrder(subOrderId: String): SubOrder
"#;
    let message = collision_error(source);
    assert!(message.contains("handle_get_sub_order"), "error: {message}");
}

#[test]
fn internal_suppression_is_not_an_exemption() {
    // `@@internal` omits the `.route(...)` registration, not the handler
    // function — so the ident still collides and must still be rejected.
    let source = r#"
model Order {
  id String @id
  total Int
  @@internal("get")
}

procedure getOrder(orderId: String): Order
"#;
    let message = collision_error(source);
    assert!(message.contains("handle_get_order"), "error: {message}");
}

#[test]
fn accepts_a_procedure_that_shares_no_generated_ident() {
    // The rename the issue reports as the workaround must keep passing.
    parse_schema(&schema_with_procedure(
        "orderDetail(orderId: String): Order",
    ))
    .expect("`orderDetail` shares no ident with any generated CRUD handler");
}

#[test]
fn accepts_a_verb_prefixed_procedure_naming_no_declared_model() {
    parse_schema(&schema_with_procedure(
        "getInvoice(invoiceId: String): Order",
    ))
    .expect("there is no `model Invoice`, so `handle_get_invoice` is unclaimed");
}

#[test]
fn accepts_a_colliding_name_when_the_schema_declares_no_model() {
    // No models means no generated CRUD handlers to collide with; a
    // procedures-only schema may name a procedure whatever it likes.
    parse_schema(
        r#"
datasource db {
  provider = "none"
}

procedure getOrder(orderId: String): String
"#,
    )
    .expect("a procedures-only schema generates no CRUD handlers");
}
