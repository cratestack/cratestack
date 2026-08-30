//! Real `flutter test` proof that the generated Dart client caches its
//! `createCborCodec()` future for SUCCESSES ONLY (cratestack#798), on both
//! transports.
//!
//! The bug: `_cratestackCborCodecFuture ??= createCborCodec()` never
//! re-evaluates once the field holds a settled *rejected* future, so a
//! single transient failure — a wasm asset that 404s on web, a vendored
//! library that was not there yet — bricked every later request in the
//! isolate, replaying the same error rather than retrying. `@cratestack/cbor
//! -web`'s `ensureInitialized()` and the generated TypeScript RPC runtime's
//! `resolveCodec()` had already been fixed for this; the two Dart runtimes
//! had not.
//!
//! **Deliberately behavioral, not source-text matching.** The TypeScript
//! suite learned this the expensive way: a prior version of its test
//! matched the literal string `"this.codecPromise ??= createCborCodec());"`,
//! which kept passing with the retry bug fully present, because it only
//! asserted the buggy line existed. Asserting `.onError<Object>(` appears
//! in the rendered template would be the same mistake in a new costume —
//! it constrains shape, and shape is not the contract. So this generates
//! two real packages, drops a stub `cratestack_cbor` whose factory can be
//! made to fail on demand under each one, and runs them under `flutter
//! test`.
//!
//! Both transports, in one test, because the cached accessor is generated
//! twice (`rest-runtime.dart.j2` and `rpc_runtime/types.dart.j2` each carry
//! their own copy) — fixing one proves nothing about the other, and the
//! repo's transport-parity rule says they ship together.
//!
//! Skips (printed, not silently swallowed) when `flutter` isn't on `PATH`,
//! matching `tests/decimal_round_trip.rs` — no Rust-only CI job in this
//! repo provisions Flutter.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

/// `(fixture stem, generated library name, driver fixture file)`. The
/// library names are load-bearing — each driver `import`s its package by
/// name, so these must match the `package:` URIs in those two files.
const TRANSPORTS: [(&str, &str, &str); 2] = [
    (
        "tiny_rest",
        "native_cbor_memo_rest",
        "native_cbor_codec_memoization_rest_test.dart",
    ),
    (
        "tiny_rpc",
        "native_cbor_memo_rpc",
        "native_cbor_codec_memoization_rpc_test.dart",
    ),
];

const COMMON_DRIVER: &str = "native_cbor_codec_memoization_common.dart";
const STUB_FIXTURE_DIR: &str = "tests/fixtures/native_cbor_codec_stub";

#[test]
fn the_generated_codec_future_is_memoized_and_retried_after_a_failure() {
    if !flutter_available() {
        eprintln!(
            "skipping the_generated_codec_future_is_memoized_and_retried_after_a_failure: \
             `flutter` not on PATH (expected in this repo's Rust-only CI jobs — see this \
             file's module doc)"
        );
        return;
    }

    for (fixture, library_name, driver) in TRANSPORTS {
        let root = project_tmp_path(&format!("native-cbor-memoization-{fixture}"));
        if root.exists() {
            fs::remove_dir_all(&root).expect("existing tmp dir should be removable");
        }
        let package_dir = root.join("package");
        write_generated_package(fixture, library_name, &package_dir);
        let stub_dir = root.join("cratestack_cbor_stub");
        copy_dir(Path::new(STUB_FIXTURE_DIR), &stub_dir);
        override_cratestack_cbor(&package_dir, &stub_dir);

        let test_dir = package_dir.join("test");
        fs::create_dir_all(&test_dir).expect("create test dir");
        for name in [COMMON_DRIVER, driver] {
            fs::copy(Path::new("tests/fixtures").join(name), test_dir.join(name))
                .unwrap_or_else(|error| panic!("copy {name} into the generated package: {error}"));
        }

        run(&package_dir, "flutter", &["pub", "get"]);
        let output = run(
            &package_dir,
            "flutter",
            &["test", &format!("test/{driver}")],
        );
        assert!(
            output.contains("All tests passed!"),
            "{fixture}: expected flutter test's own success marker, got:\n{output}"
        );

        fs::remove_dir_all(&root).expect("tmp dir should be removable");
    }
}

fn write_generated_package(fixture: &str, library_name: &str, dir: &Path) {
    let fixture_path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            // The whole point — the cached codec accessor only exists on
            // the native path.
            native_cbor: true,
        },
    )
    .unwrap_or_else(|error| panic!("{fixture}: generation should succeed: {error}"));

    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }
}

/// Points the generated package's `cratestack_cbor` dependency at the stub.
///
/// A `pubspec_overrides.yaml` rather than an edit to the generated
/// `pubspec.yaml`, for the same reason `justfile`'s
/// `override_cratestack_cbor_for_verification` uses one: the generated file
/// stays byte-for-byte what the generator emitted, so nothing here can mask
/// a regression in what it emits.
fn override_cratestack_cbor(package_dir: &Path, stub_dir: &Path) {
    let stub_path = stub_dir
        .canonicalize()
        .expect("stub dir should exist by now");
    // `cratestack_builder`/`cratestack_annotations` are overridden to this
    // repo's own `dart-packages/` in the SAME file, because pub allows only
    // one `pubspec_overrides.yaml` per package. Without them, a commit that
    // raises the generator's annotations floor and the builder's own
    // `cratestack_annotations` constraint together cannot resolve here: the
    // builder pub.dev still serves forbids the annotations release the
    // generator now asks for. See `package_floors.rs`'s module doc on the
    // chicken-and-egg in the lockstep publishing model.
    let dart_packages = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dart-packages")
        .canonicalize()
        .expect("dart-packages/ should exist in this repo");
    fs::write(
        package_dir.join("pubspec_overrides.yaml"),
        format!(
            "dependency_overrides:\n  \
             cratestack_cbor:\n    path: {}\n  \
             cratestack_annotations:\n    path: {}/cratestack_annotations\n  \
             cratestack_builder:\n    path: {}/cratestack_builder\n",
            stub_path.display(),
            dart_packages.display(),
            dart_packages.display()
        ),
    )
    .expect("write pubspec_overrides.yaml");
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create destination dir");
    for entry in fs::read_dir(from).unwrap_or_else(|error| panic!("read {from:?}: {error}")) {
        let entry = entry.expect("read dir entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

fn run(dir: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|error| panic!("run {program} {args:?}: {error}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{program} {args:?} failed in {dir:?}:\nstdout: {stdout}\nstderr: {stderr}"
    );
    format!("{stdout}{stderr}")
}

fn flutter_available() -> bool {
    Command::new("flutter")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn project_tmp_path(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/client-dart-tests")
        .join(format!("{label}-{suffix}"))
}
