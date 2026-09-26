//! cratestack#1074: migrate and codegen agree on the primary key because
//! both go through `cratestack_core::Field::is_primary_key`. An attribute
//! that merely starts with `@id` is an ordinary (unknown, inert) attribute
//! here too, not a second primary-key column.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn an_id_prefixed_attribute_is_not_a_primary_key_column() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  code String @identity
}
"#,
    ));
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
