//! Parser tests for `view` blocks (ADR-0003).

use crate::parse_schema;

const BASIC_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
}

model Customer {
  id Int @id
  email String
}

model Order {
  id Int @id
  customerId Int
  createdAt DateTime
}

view ActiveCustomer from Customer, Order {
  id Int @id @from(Customer.id)
  email String @from(Customer.email)
  orderCount Int

  @@server_sql("SELECT c.id, c.email, COUNT(o.id) AS order_count FROM customer c LEFT JOIN \"order\" o ON o.customer_id = c.id GROUP BY c.id, c.email")
  @@embedded_sql("SELECT c.id, c.email, COUNT(o.id) AS order_count FROM customer c LEFT JOIN \"order\" o ON o.customer_id = c.id GROUP BY c.id, c.email")

  @@allow("read", auth() != null)
}
"#;

#[test]
fn parses_basic_view() {
    let schema = parse_schema(BASIC_SCHEMA).expect("schema parses");
    assert_eq!(schema.views.len(), 1);

    let view = &schema.views[0];
    assert_eq!(view.name, "ActiveCustomer");
    assert_eq!(view.sources.len(), 2);
    assert_eq!(view.sources[0].name, "Customer");
    assert_eq!(view.sources[1].name, "Order");
    assert_eq!(view.fields.len(), 3);
    assert!(view.server_sql().is_some());
    assert!(view.embedded_sql().is_some());
    assert!(!view.is_materialized());
    assert!(!view.no_unique());
}

#[test]
fn server_only_view_omits_embedded_sql() {
    let schema = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view ServerOnly from Customer {
  id Int @id @from(Customer.id)
  @@server_sql("SELECT id FROM customer")
}
"#,
    )
    .expect("schema parses");
    assert!(schema.views[0].server_sql().is_some());
    assert!(schema.views[0].embedded_sql().is_none());
}

#[test]
fn materialized_view_parses() {
    let schema = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view CustomerSummary from Customer {
  id Int @id @from(Customer.id)
  @@server_sql("SELECT id FROM customer")
  @@materialized
}
"#,
    )
    .expect("schema parses");
    assert!(schema.views[0].is_materialized());
}

#[test]
fn rejects_unknown_source_model() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view Bogus from DoesNotExist {
  id Int @id @from(DoesNotExist.id)
  @@server_sql("SELECT 1")
}
"#,
    )
    .expect_err("should reject unknown source");
    assert!(
        err.message().contains("unknown source model"),
        "unexpected error: {}",
        err.message()
    );
}

#[test]
fn rejects_view_with_no_sql_body() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view NoSql from Customer {
  id Int @id @from(Customer.id)
}
"#,
    )
    .expect_err("should reject view with no SQL body");
    assert!(
        err.message().contains("must declare a SQL body"),
        "unexpected error: {}",
        err.message()
    );
}

#[test]
fn rejects_materialized_with_no_unique() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view Bad from Customer {
  email String
  @@server_sql("SELECT email FROM customer")
  @@materialized
  @@no_unique
}
"#,
    )
    .expect_err("should reject materialized + no_unique");
    assert!(
        err.message().contains("cannot be both `@@materialized` and `@@no_unique`"),
        "unexpected error: {}",
        err.message()
    );
}

#[test]
fn rejects_allow_with_non_read_action() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view Bad from Customer {
  id Int @id @from(Customer.id)
  @@server_sql("SELECT id FROM customer")
  @@allow("create", true)
}
"#,
    )
    .expect_err("should reject non-read @@allow on view");
    assert!(
        err.message().contains("only supports the `read` action"),
        "unexpected error: {}",
        err.message()
    );
}

#[test]
fn rejects_view_with_no_id_and_no_unique_opt_out() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view Bad from Customer {
  email String
  @@server_sql("SELECT email FROM customer")
}
"#,
    )
    .expect_err("should reject missing @id");
    assert!(
        err.message().contains("must declare exactly one `@id` field"),
        "unexpected error: {}",
        err.message()
    );
}

#[test]
fn no_unique_view_skips_id_requirement() {
    let schema = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}
model Customer {
  id Int @id
}
view NoKey from Customer {
  email String
  @@server_sql("SELECT email FROM customer")
  @@no_unique
}
"#,
    )
    .expect("schema parses");
    assert!(schema.views[0].no_unique());
}
