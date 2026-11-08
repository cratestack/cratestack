//! The file graph each service is loaded from, which is what `cratestack
//! check --list-files` prints, and the rule that keeps an import to shapes:
//! a shared file that declares a `model` is refused.

use std::path::{Path, PathBuf};

use cratestack_parser::{FileRole, LoadOptions, OverlaySources, load_schema};

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn root(service: &str) -> PathBuf {
    manifest().join(service).join("schema.cstack")
}

fn files(service: &str) -> Vec<(FileRole, String)> {
    let loaded = load_schema(root(service), &LoadOptions::default()).expect("the schema loads");
    loaded
        .files
        .iter()
        .map(|file| (file.role, file.relative.clone()))
        .collect()
}

fn listed(entries: &[(FileRole, &str)]) -> Vec<(FileRole, String)> {
    entries
        .iter()
        .map(|(role, path)| (*role, (*path).to_owned()))
        .collect()
}

#[test]
fn billing_reaches_currency_only_through_common() {
    assert_eq!(
        files("billing"),
        listed(&[
            (FileRole::Root, "schema.cstack"),
            (FileRole::Import, "../shared/common.cstack"),
            (FileRole::Import, "../shared/currency.cstack"),
        ])
    );
}

#[test]
fn shipping_loads_the_diamond_once() {
    assert_eq!(
        files("shipping"),
        listed(&[
            (FileRole::Root, "schema.cstack"),
            (FileRole::Import, "../shared/currency.cstack"),
            (FileRole::Import, "../shared/common.cstack"),
        ])
    );
}

/// A shared file is a schema on its own too: it loads without either service.
#[test]
fn the_shared_file_loads_on_its_own() {
    let loaded = load_schema(
        manifest().join("shared/common.cstack"),
        &LoadOptions::default(),
    )
    .expect("common.cstack loads");
    assert!(loaded.schema.models.is_empty());
    assert_eq!(loaded.files.len(), 2);
}

/// Imports carry shapes, never tables (M12 in the design's message table).
/// The shared file is changed in memory only: nothing on disk changes.
#[test]
fn a_model_in_a_shared_file_is_refused() {
    let common = manifest().join("shared/common.cstack");
    let original = std::fs::read_to_string(&common).expect("the shared file exists");
    let mut sources = OverlaySources::new(true);
    sources.insert(
        &common,
        format!("{original}\nmodel Customer {{\n  id Int @id\n}}\n"),
    );
    let options = LoadOptions {
        follow_part_of: false,
        sources: Some(&sources),
    };
    let error = load_schema(root("billing"), &options).expect_err("the import is refused");
    assert_eq!(
        error.message(),
        "`model Customer` cannot come from an import: `../shared/common.cstack` is imported by \
         `schema.cstack`, and an imported file may declare only `type`, `enum` and `mixin`; use \
         `part` to split one schema across files"
    );
}
