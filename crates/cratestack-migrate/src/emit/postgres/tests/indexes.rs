//! Postgres DDL for the general model-level `@@index([...], using: ...,
//! opclass: "...")` attribute (issue #156 — pgvector phase 2). Unlike
//! `@@unique([...])` (see `super::uniques`), an `@@index` is never
//! unique, and its `using`/`opclass` fields are optional passthrough —
//! deliberately *not* gated on the `pgvector` Cargo feature, since the
//! attribute itself isn't pgvector-specific (see
//! `docs/design/extensions.md` §8 item 5). The `pgvector`-gated tests at
//! the bottom exercise the concrete ivfflat/hnsw ANN-index use case this
//! ticket exists for, over a real `Vector(n)` column.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn bare_index_emits_plain_non_unique_create_index() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Order {
  id String @id
  customerEmail String

  @@index([customerEmail])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("CREATE INDEX orders_customer_email_idx ON orders (customer_email);"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("UNIQUE"),
        "a bare @@index must not be unique: {}",
        migration.up
    );
}

#[test]
fn index_using_renders_generic_access_method_without_pgvector() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Document {
  id String @id
  body String

  @@index([body], using: gin)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("CREATE INDEX documents_body_gin_idx ON documents USING gin (body);"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn bare_and_specialized_index_over_the_same_column_coexist() {
    let prev = schema(&with_models(
        r#"
model Document {
  id String @id
  body String

  @@index([body])
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Document {
  id String @id
  body String

  @@index([body])
  @@index([body], using: gin)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("CREATE INDEX documents_body_gin_idx ON documents USING gin (body);"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("DROP INDEX documents_body_idx"),
        "the pre-existing bare index must be untouched: {}",
        migration.up
    );
}

#[test]
fn dropping_index_emits_drop_index() {
    let prev = schema(&with_models(
        r#"
model Order {
  id String @id
  customerEmail String

  @@index([customerEmail])
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Order {
  id String @id
  customerEmail String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("DROP INDEX orders_customer_email_idx;"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn existing_unique_index_ddl_is_unaffected_by_the_new_ir_fields() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Customer {
  id String @id
  email String @unique
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("CREATE UNIQUE INDEX customers_email_key ON customers (email);"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("USING"), "up was: {}", migration.up);
}

#[cfg(feature = "pgvector")]
#[test]
fn ivfflat_index_with_opclass_renders_ann_index_ddl() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
extension pgvector {
}

model Document {
  id String @id
  embedding Vector(3)

  @@index([embedding], using: ivfflat, opclass: "vector_l2_ops")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE INDEX documents_embedding_ivfflat_idx ON documents \
             USING ivfflat (embedding vector_l2_ops);"
        ),
        "up was: {}",
        migration.up
    );
}

#[cfg(feature = "pgvector")]
#[test]
fn hnsw_index_with_opclass_renders_ann_index_ddl() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
extension pgvector {
}

model Document {
  id String @id
  embedding Vector(3)

  @@index([embedding], using: hnsw, opclass: "vector_cosine_ops")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE INDEX documents_embedding_hnsw_idx ON documents \
             USING hnsw (embedding vector_cosine_ops);"
        ),
        "up was: {}",
        migration.up
    );
}
