// Golden-file snapshot tests for `--preset riverpod` (issue #301).
// Mirrors `tests/snapshot.rs`'s harness/`CRATESTACK_UPDATE_SNAPSHOTS=1`
// convention, kept in its own file so `tests/snapshot.rs`'s default-preset
// snapshots stay untouched, per this story's byte-identical requirement.

use std::fs;
use std::path::{Path, PathBuf};

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const SNAPSHOT_SCHEMA_SHA256: &str =
    "9f1c1e3b6b7f27e0d2a5b1c4e8f0a3d6c9b2e5f8a1d4c7b0e3f6a9c2d5b8e1f4";

#[test]
fn riverpod_rest_snapshot_matches_fixture() {
    run_snapshot("tiny_rest", "tiny_rest_client");
}

#[test]
fn riverpod_rpc_snapshot_matches_fixture() {
    run_snapshot("tiny_rpc", "tiny_rpc_client");
}

fn run_snapshot(fixture_stem: &str, library_name: &str) {
    let package = generate_for(fixture_stem, library_name);
    let snapshot_dir = snapshot_root().join(format!("riverpod_{fixture_stem}"));
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

fn generate_for(fixture_stem: &str, library_name: &str) -> GeneratedDartPackage {
    let fixture_path = fixture_root().join(format!("{fixture_stem}.cstack"));
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Riverpod,
            pb_lock: None,
            schema_sha256: SNAPSHOT_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("riverpod template should render")
}

fn write_snapshot(dir: &Path, package: &GeneratedDartPackage) {
    if dir.exists() {
        fs::remove_dir_all(dir).expect("snapshot dir should be removable");
    }
    fs::create_dir_all(dir).expect("snapshot dir should be creatable");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("snapshot subdir should be creatable");
        }
        fs::write(&path, file.contents.as_bytes()).expect("snapshot file should write");
    }
}

fn assert_snapshot_matches(dir: &Path, package: &GeneratedDartPackage) {
    assert!(
        dir.exists(),
        "snapshot directory {dir:?} is missing — run `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-dart` to create it"
    );
    for file in &package.files {
        let path = dir.join(&file.file_name);
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "snapshot file {path:?} is missing — run with CRATESTACK_UPDATE_SNAPSHOTS=1 to create it ({error})"
            )
        });
        assert_eq!(
            file.contents, expected,
            "snapshot mismatch for {} — run CRATESTACK_UPDATE_SNAPSHOTS=1 to refresh",
            file.file_name
        );
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}
