use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn enum_create_emits_create_type_and_uses_snake_case() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
enum OrderStatus {
  Pending
  Submitted
  Shipped
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy);
    // Enum type DDL lands before the table that references it.
    let create_type_idx = migration
        .up
        .find("CREATE TYPE order_status AS ENUM")
        .expect("CREATE TYPE present");
    let create_table_idx = migration
        .up
        .find("CREATE TABLE orders")
        .expect("CREATE TABLE present");
    assert!(
        create_type_idx < create_table_idx,
        "CREATE TYPE must precede CREATE TABLE so the column can reference the enum"
    );
    // Column type references the snake-cased enum.
    assert!(
        migration.up.contains("status order_status NOT NULL"),
        "up was: {}",
        migration.up
    );
    // Variants are single-quoted.
    assert!(migration.up.contains("'Pending', 'Submitted', 'Shipped'"));
}

#[test]
fn enum_add_variant_emits_alter_type_add_value() {
    let prev = schema(&with_models(
        r#"
enum OrderStatus {
  Pending
  Submitted
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
    ));
    let next = schema(&with_models(
        r#"
enum OrderStatus {
  Pending
  Submitted
  Shipped
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy);
    assert!(
        migration
            .up
            .contains("ALTER TYPE order_status ADD VALUE 'Shipped';"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn enum_drop_is_lossy_and_routes_to_error_stub() {
    let prev = schema(&with_models(
        r#"
enum LegacyStatus {
  Active
}
"#,
    ));
    let next = schema(&with_models(""));
    let migration = emit(&diff(&prev, &next));
    assert!(migration.has_lossy);
    assert!(
        migration.up.contains("DROP TYPE legacy_status;"),
        "up was: {}",
        migration.up
    );
    assert!(migration.down.contains("destructive migration"));
}
