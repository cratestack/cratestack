//! The load-bearing test of multi-file schemas (docs/design/multi-file-schemas.md
//! section 8): a schema split across files is the IR of its hand-written
//! concatenation, source positions aside. The fixtures are real files, read
//! from disk.

use std::path::PathBuf;

use cratestack_core::{Schema, schema::without_source_positions, schema_digest_hex};
use cratestack_parser::{LoadOptions, load_schema, parse_schema_file};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/multi_file")
        .join(relative)
}

fn load(relative: &str) -> Schema {
    load_schema(fixture(relative), &LoadOptions::default())
        .unwrap_or_else(|e| panic!("{relative}: {}", e.render()))
        .schema
}

fn json(schema: &Schema) -> Vec<u8> {
    serde_json::to_vec_pretty(&without_source_positions(schema)).expect("serializes")
}

#[test]
fn split_and_concatenated_ir_are_byte_identical() {
    let split = load("split/schema.cstack");
    let whole = load("concatenated.cstack");
    assert_eq!(whole.models.len(), 2, "the fixture is not trivially empty");
    assert!(
        json(&split) == json(&whole),
        "the split IR differs from the concatenated IR"
    );
}

#[test]
fn split_and_concatenated_have_the_same_schema_digest() {
    let split = load("split/schema.cstack");
    let whole = load("concatenated.cstack");
    assert_eq!(schema_digest_hex(&split), schema_digest_hex(&whole));
}

#[test]
fn print_ir_no_spans_debug_is_identical() {
    let split = without_source_positions(&load("split/schema.cstack"));
    let whole = without_source_positions(&load("concatenated.cstack"));
    assert_eq!(format!("{split:#?}"), format!("{whole:#?}"));
}

#[test]
fn a_perturbed_split_is_detected() {
    let perturbed = load("perturbed/schema.cstack");
    let whole = load("concatenated.cstack");
    assert!(
        json(&perturbed) != json(&whole),
        "swapping two models must change the IR"
    );
    let order: Vec<_> = perturbed.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(order, ["Post", "User"]);
}

#[test]
fn parse_schema_file_loads_the_parts_too() {
    let via_file = parse_schema_file(fixture("split/schema.cstack")).expect("loads");
    assert_eq!(via_file, load("split/schema.cstack"));
}

#[test]
fn spans_of_a_split_schema_are_unique_across_files() {
    let split = load("split/schema.cstack");
    let mut starts: Vec<_> = split.models.iter().map(|m| m.span.start).collect();
    starts.sort_unstable();
    starts.dedup();
    assert_eq!(starts.len(), split.models.len());
    assert!(split.models[1].span.start > split.models[0].span.start);
}

#[test]
fn the_split_fixture_loads_in_design_order_with_the_diamond_once() {
    let loaded = load_schema(fixture("split/schema.cstack"), &LoadOptions::default()).unwrap();
    let order: Vec<_> = loaded
        .files
        .iter()
        .map(|f| (f.relative.as_str(), f.role))
        .collect();
    use cratestack_parser::FileRole::{Import, ImportPart, Part, Root};
    // `shared/ids.cstack` is imported by two parts and loaded once, after the
    // root's own import, at the first import in load order.
    assert_eq!(
        order,
        [
            ("schema.cstack", Root),
            ("models/user.cstack", Part),
            ("models/post.cstack", Part),
            ("api.cstack", Part),
            ("shared/money.cstack", Import),
            ("shared/money/currency.cstack", ImportPart),
            ("shared/ids.cstack", Import),
        ]
    );
}
