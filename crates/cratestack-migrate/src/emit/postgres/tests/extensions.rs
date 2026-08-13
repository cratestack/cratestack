//! `pgvector` DDL emission: `CREATE EXTENSION IF NOT EXISTS vector;`
//! plus `vector(n)` column rendering. The positive-emission tests only
//! run when this crate is built with the `pgvector` feature (the DDL
//! is a hard panic otherwise, by design — see
//! `super::super::extensions::emit_ensure_extension` /
//! `super::super::columns::render_vector_type`); the negative test at
//! the bottom exercises that panic directly, without the feature, to
//! prove the gate is real rather than dormant.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[cfg(feature = "pgvector")]
#[test]
fn create_extension_emitted_once_before_column_ddl() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(1536)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));

    let extension_pos = migration
        .up
        .find("CREATE EXTENSION IF NOT EXISTS vector;")
        .expect("CREATE EXTENSION should be emitted");
    let column_pos = migration
        .up
        .find("vector(1536)")
        .expect("vector(1536) column DDL should be emitted");
    assert!(
        extension_pos < column_pos,
        "CREATE EXTENSION must precede column DDL referencing it: {}",
        migration.up
    );
    assert_eq!(
        migration
            .up
            .matches("CREATE EXTENSION IF NOT EXISTS vector;")
            .count(),
        1,
        "CREATE EXTENSION should appear exactly once: {}",
        migration.up
    );
}

#[cfg(feature = "pgvector")]
#[test]
fn vector_column_renders_parametric_type_not_text() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(3)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains("embedding vector(3) NOT NULL"),
        "embedding column should render as vector(3), not TEXT: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("embedding TEXT"),
        "embedding column must not silently fall back to TEXT: {}",
        migration.up
    );
}

#[cfg(feature = "pgvector")]
#[test]
fn redeclaring_pgvector_does_not_reemit_create_extension() {
    let prev = schema(&with_models(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(3)
}
"#,
    ));
    let next = schema(&with_models(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(3)
  embedding2 Vector(3)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        !migration.up.contains("CREATE EXTENSION"),
        "an already-declared extension shouldn't be re-emitted on a later diff: {}",
        migration.up
    );
    assert!(migration.up.contains("embedding2"));
}

#[cfg(not(feature = "pgvector"))]
#[test]
#[should_panic(expected = "pgvector")]
fn vector_ddl_panics_without_pgvector_feature() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(3)
}
"#,
    ));
    // Building `cratestack-migrate` without the `pgvector` feature and
    // then feeding it a schema that declares the extension (which the
    // parser only allows alongside a real `Vector(n)` usage) is a hard
    // panic, not silently-wrong DDL — mirrors `cratestack-macros`'
    // `compile_error!` gate (#161), just enforced at runtime here
    // since this crate isn't a proc-macro.
    let _ = emit(&diff(&prev, &next).expect("diff should succeed"));
}
