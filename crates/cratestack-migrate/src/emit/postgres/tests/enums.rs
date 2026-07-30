//! Enum-typed columns are stored as `TEXT` plus a membership `CHECK`,
//! not as a native `CREATE TYPE ... AS ENUM` (issue #228), and their
//! bareword `@default(...)` literals are quoted (issue #227).

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

fn order_schema(variants: &str, field: &str) -> String {
    with_models(&format!(
        r#"
enum OrderStatus {{
{variants}
}}

model Order {{
  id Int @id
  {field}
}}
"#
    ))
}

#[test]
fn enum_column_is_text_with_membership_check_and_no_create_type() {
    let prev = schema(&with_models(""));
    let next = schema(&order_schema(
        "  Pending\n  Submitted\n  Shipped",
        "status OrderStatus",
    ));
    let migration = emit(&diff(&prev, &next));

    // The native enum type is gone entirely — this is the storage
    // representation the generated row decoder actually reads.
    assert!(
        !migration.up.contains("CREATE TYPE"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains("status TEXT NOT NULL"),
        "up was: {}",
        migration.up
    );

    // The CHECK recovers the validation the native type provided, and
    // must land after the table it constrains exists.
    let create_table_idx = migration
        .up
        .find("CREATE TABLE orders")
        .expect("CREATE TABLE present");
    let check_idx = migration
        .up
        .find("ADD CONSTRAINT orders_status_enum_check")
        .expect("enum CHECK present");
    assert!(
        create_table_idx < check_idx,
        "CHECK must follow the CREATE TABLE it constrains; up was: {}",
        migration.up
    );
    assert!(
        migration
            .up
            .contains("CHECK (status IN ('Pending', 'Submitted', 'Shipped'))"),
        "up was: {}",
        migration.up
    );

    // Creating a table plus its enum CHECK touches no existing rows.
    assert!(!migration.has_lossy);
    assert!(
        !migration.has_blocking,
        "an enum CHECK on a freshly created table cannot block; up was: {}",
        migration.up
    );
}

/// Issue #227: `@default(pending)` is a bareword in the `.cstack`
/// source. Emitted unquoted, Postgres parses it as a column reference
/// and rejects the statement with "cannot use column reference in
/// DEFAULT expression".
#[test]
fn bareword_enum_default_is_quoted() {
    let prev = schema(&with_models(""));
    let next = schema(&order_schema(
        "  Pending\n  Shipped",
        "status OrderStatus @default(Pending)",
    ));
    let migration = emit(&diff(&prev, &next));

    assert!(
        migration
            .up
            .contains("status TEXT NOT NULL DEFAULT 'Pending'"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("DEFAULT Pending"),
        "bareword default must not survive into DDL; up was: {}",
        migration.up
    );
}

#[test]
fn optional_enum_column_is_nullable_text_and_still_checked() {
    let prev = schema(&with_models(""));
    let next = schema(&order_schema("  Pending\n  Shipped", "status OrderStatus?"));
    let migration = emit(&diff(&prev, &next));

    assert!(
        migration.up.contains("status TEXT,") || migration.up.contains("status TEXT\n"),
        "optional enum must be nullable TEXT; up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("status TEXT NOT NULL"));
    // `NULL IN (...)` is NULL, and a CHECK only fails on FALSE, so the
    // plain membership predicate already admits NULL.
    assert!(
        migration
            .up
            .contains("CHECK (status IN ('Pending', 'Shipped'))"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn enum_list_column_is_text_array_with_containment_check() {
    let prev = schema(&with_models(""));
    let next = schema(&order_schema(
        "  Pending\n  Shipped",
        "history OrderStatus[]",
    ));
    let migration = emit(&diff(&prev, &next));

    assert!(
        migration.up.contains("history TEXT[] NOT NULL"),
        "up was: {}",
        migration.up
    );
    // Array containment, not scalar membership — the generated
    // decoder reads this column as `Vec<String>`.
    assert!(
        migration
            .up
            .contains("CHECK (history <@ ARRAY['Pending', 'Shipped']::TEXT[])"),
        "up was: {}",
        migration.up
    );
}

/// The path that replaces `ALTER TYPE ... ADD VALUE`. Adding a variant
/// must produce a working migration: drop the old CHECK, add the
/// widened one.
#[test]
fn adding_a_variant_rebuilds_the_membership_check() {
    let prev = schema(&order_schema(
        "  Pending\n  Submitted",
        "status OrderStatus",
    ));
    let next = schema(&order_schema(
        "  Pending\n  Submitted\n  Shipped",
        "status OrderStatus",
    ));
    let migration = emit(&diff(&prev, &next));

    assert!(
        !migration.up.contains("ALTER TYPE"),
        "up was: {}",
        migration.up
    );
    let drop_idx = migration
        .up
        .find("DROP CONSTRAINT orders_status_enum_check")
        .expect("old CHECK dropped");
    let add_idx = migration
        .up
        .find("ADD CONSTRAINT orders_status_enum_check")
        .expect("widened CHECK added");
    assert!(
        drop_idx < add_idx,
        "the old CHECK must be dropped before the new one is added; up was: {}",
        migration.up
    );
    assert!(
        migration
            .up
            .contains("CHECK (status IN ('Pending', 'Submitted', 'Shipped'))"),
        "up was: {}",
        migration.up
    );

    // Widening cannot reject a row that already passed, and unlike
    // `ALTER TYPE ... ADD VALUE` this runs inside a transaction.
    assert!(!migration.has_lossy);
    assert!(!migration.has_blocking);
}

/// Native Postgres enums cannot drop a variant at all, so the old
/// emitter ignored removals. Under the CHECK model the constraint is
/// simply rebuilt narrower.
#[test]
fn removing_a_variant_rebuilds_the_check_narrower() {
    let prev = schema(&order_schema(
        "  Pending\n  Submitted\n  Shipped",
        "status OrderStatus",
    ));
    let next = schema(&order_schema(
        "  Pending\n  Submitted",
        "status OrderStatus",
    ));
    let migration = emit(&diff(&prev, &next));

    assert!(
        migration
            .up
            .contains("CHECK (status IN ('Pending', 'Submitted'))"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("'Shipped'"),
        "up was: {}",
        migration.up
    );
}

/// Removing the enum declaration no longer emits `DROP TYPE`, so a
/// migration that only removes an unused enum is not destructive —
/// there is genuinely nothing left to drop.
#[test]
fn dropping_an_unused_enum_declaration_emits_nothing() {
    let prev = schema(&with_models(
        r#"
enum LegacyStatus {
  Active
}
"#,
    ));
    let next = schema(&with_models(""));
    let migration = emit(&diff(&prev, &next));

    assert!(
        !migration.up.contains("DROP TYPE"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.has_lossy);
}

/// The enum CHECK is reversible: `down` drops the constraint the `up`
/// added, and the migration is not routed to the destructive stub.
#[test]
fn enum_check_is_reversible() {
    let prev = schema(&with_models(""));
    let next = schema(&order_schema("  Pending\n  Shipped", "status OrderStatus"));
    let migration = emit(&diff(&prev, &next));

    assert!(!migration.down.contains("destructive migration"));
    assert!(
        migration
            .down
            .contains("DROP CONSTRAINT orders_status_enum_check"),
        "down was: {}",
        migration.down
    );
}
