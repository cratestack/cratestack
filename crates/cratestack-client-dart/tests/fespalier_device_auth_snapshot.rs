// Golden-file snapshot tests for the `DeviceAuthServer` that `--preset
// fespalier` emits for a schema with device-auth markers (cratestack#1303),
// REST and RPC. Same harness and `CRATESTACK_UPDATE_SNAPSHOTS=1` convention
// as `tests/fespalier_snapshot.rs`; the fixtures are fespalier's
// `examples/cratestack` ceremonies (see their header).
//
// The behavioural proof (request bodies, mapping, 403, `fieldErrors`,
// signed revoke, abort isolation) is `just verify-dart-fespalier`, which
// runs `tests/fixtures/fespalier_device_auth_{rest,rpc}_test.dart`.

use std::fs;
use std::path::{Path, PathBuf};

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const SNAPSHOT_SCHEMA_SHA256: &str =
    "9f1c1e3b6b7f27e0d2a5b1c4e8f0a3d6c9b2e5f8a1d4c7b0e3f6a9c2d5b8e1f4";

#[test]
fn device_auth_rpc_snapshot_matches_fixture() {
    run_snapshot("device_auth_rpc", "device_auth_rpc_client");
}

#[test]
fn device_auth_rest_snapshot_matches_fixture() {
    run_snapshot("device_auth_rest", "device_auth_rest_client");
}

fn run_snapshot(fixture_stem: &str, library_name: &str) {
    let fixture_path = fixture_root().join(format!("{fixture_stem}.cstack"));
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Fespalier,
            schema_sha256: SNAPSHOT_SCHEMA_SHA256.to_owned(),
            ..DartGeneratorConfig::default()
        },
    )
    .expect("fespalier template should render");
    let snapshot_dir = snapshot_root().join(format!("fespalier_{fixture_stem}"));
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
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
        "snapshot directory {dir:?} is missing: run `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-dart` to create it"
    );
    for file in &package.files {
        let path = dir.join(&file.file_name);
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "snapshot file {path:?} is missing: run with CRATESTACK_UPDATE_SNAPSHOTS=1 to create it ({error})"
            )
        });
        assert_eq!(
            file.contents, expected,
            "snapshot mismatch for {}: run CRATESTACK_UPDATE_SNAPSHOTS=1 to refresh",
            file.file_name
        );
    }
    let on_disk = count_files(dir);
    assert_eq!(
        on_disk,
        package.files.len(),
        "the snapshot has files the generator no longer emits"
    );
}

fn count_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .expect("snapshot dir should be readable")
        .map(|entry| entry.expect("entry").path())
        .map(|path| if path.is_dir() { count_files(&path) } else { 1 })
        .sum()
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}
