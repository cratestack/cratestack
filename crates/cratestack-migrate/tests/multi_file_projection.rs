//! Splitting a schema across files must not change what `migrate diff` sees:
//! the projection of the split schema is the projection of its hand-written
//! concatenation (docs/design/multi-file-schemas.md section 8).

use std::path::PathBuf;

use cratestack_migrate::project;
use cratestack_parser::{LoadOptions, load_schema};

fn load(relative: &str) -> cratestack_core::Schema {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../cratestack-parser/tests/fixtures/multi_file")
        .join(relative);
    load_schema(&path, &LoadOptions::default())
        .unwrap_or_else(|e| panic!("{relative}: {}", e.render()))
        .schema
}

#[test]
fn project_of_the_split_equals_project_of_the_concatenation() {
    let split = project(&load("split/schema.cstack"));
    let whole = project(&load("concatenated.cstack"));
    assert_eq!(split.tables.len(), 2, "the fixture projects real tables");
    assert_eq!(split, whole);
    assert_eq!(
        serde_json::to_string(&split).unwrap(),
        serde_json::to_string(&whole).unwrap()
    );
}
