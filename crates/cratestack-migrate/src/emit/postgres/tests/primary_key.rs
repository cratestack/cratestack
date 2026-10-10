//! cratestack#1074: migrate and codegen agree on the primary key because
//! both go through `cratestack_core::Field::is_primary_key`. An attribute
//! that merely starts with `@id` is not a second primary-key column here
//! either. The parser now refuses such a name outright (ADR 0019 D5), so the
//! schema is parsed without validation: this is the emitter's own guard, for
//! a shape that can still arrive through an old snapshot.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn an_id_prefixed_attribute_is_not_a_primary_key_column() {
    let prev = schema(&with_models(""));
    let next = cratestack_parser::parse_schema_unvalidated(&with_models(
        r#"
model Account {
  id Int @id
  code String @identity
}
"#,
    ))
    .expect("syntactically valid");
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains("PRIMARY KEY (id)"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("PRIMARY KEY (id, code)"),
        "up was: {}",
        migration.up
    );
}
