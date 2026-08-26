//! Postgres DDL for the `where: "<sql predicate>"` keyword argument on
//! `@@unique([...])` and `@@index([...])` (issue #742 — partial
//! indexes). The round-trip-through-introspection requirement (the
//! ticket's central risk) is proved against a live Postgres in
//! `crates/cratestack-migrate/tests/postgres_introspect.rs`, not here —
//! these are pure-IR emit/diff assertions.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

const IDEMPOTENCY_MODEL: &str = r#"
model Payment {
  id String @id
  idempotencyKey String?
  amount Int

  @@unique([idempotencyKey], where: "idempotency_key IS NOT NULL")
}
"#;

#[test]
fn composite_unique_with_where_emits_partial_unique_index() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(IDEMPOTENCY_MODEL));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX payments_idempotency_key_key ON payments (idempotency_key) \
             WHERE idempotency_key IS NOT NULL;"
        ),
        "up was: {}",
        migration.up
    );
}

#[test]
fn composite_unique_with_where_is_reversed_by_a_plain_drop_index() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(IDEMPOTENCY_MODEL));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration
            .down
            .contains("DROP INDEX payments_idempotency_key_key;"),
        "down was: {}",
        migration.down
    );
}

#[test]
fn index_with_where_emits_partial_non_unique_index() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Order {
  id String @id
  status String

  @@index([status], where: "status = 'active'")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("CREATE INDEX orders_status_idx ON orders (status) WHERE status = 'active';"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("UNIQUE"),
        "a bare @@index must not be unique: {}",
        migration.up
    );
}

/// Acceptance criterion: no `where:` ⇒ byte-identical DDL to before this
/// field existed — no trailing `WHERE`, no accidental space before the
/// terminating `;`.
#[test]
fn no_where_predicate_renders_byte_identical_ddl() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
  @@index([name])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX applications_tenant_id_name_key ON applications (tenant_id, name);"
        ),
        "up was: {}",
        migration.up
    );
    assert!(
        migration
            .up
            .contains("CREATE INDEX applications_name_idx ON applications (name);"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("WHERE"), "up was: {}", migration.up);
}

/// Acceptance criterion: a changed predicate is a drop + recreate, not
/// an in-place alter (neither backend supports one).
#[test]
fn changing_the_where_predicate_drops_and_recreates_the_index() {
    let prev = schema(&with_models(IDEMPOTENCY_MODEL));
    let next = schema(&with_models(
        r#"
model Payment {
  id String @id
  idempotencyKey String?
  amount Int

  @@unique([idempotencyKey], where: "idempotency_key IS NOT NULL AND amount > 0")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("DROP INDEX payments_idempotency_key_key;"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX payments_idempotency_key_key ON payments (idempotency_key) \
             WHERE idempotency_key IS NOT NULL AND amount > 0;"
        ),
        "up was: {}",
        migration.up
    );
}

/// The predicate-change diff must not fire on every plan once the
/// schema is stable — same (prev, prev) diff as always is a no-op.
#[test]
fn unchanged_where_predicate_produces_no_ops() {
    let source = with_models(IDEMPOTENCY_MODEL);
    let s = schema(&source);
    assert!(diff(&s, &s).expect("diff should succeed").is_empty());
}

/// A predicate that only differs by whitespace/an already-present outer
/// paren (the shape Postgres's own `pg_get_expr` would normalize a
/// stored predicate into) must NOT be treated as a change — this is the
/// pure-IR half of the churn-prevention requirement; the live-Postgres
/// half is `postgres_introspect.rs`.
#[test]
fn a_predicate_that_only_differs_by_normalization_produces_no_ops() {
    let prev = schema(&with_models(IDEMPOTENCY_MODEL));
    let next = schema(&with_models(
        r#"
model Payment {
  id String @id
  idempotencyKey String?
  amount Int

  @@unique([idempotencyKey], where: "(idempotency_key   IS  NOT NULL)")
}
"#,
    ));
    assert!(diff(&prev, &next).expect("diff should succeed").is_empty());
}
