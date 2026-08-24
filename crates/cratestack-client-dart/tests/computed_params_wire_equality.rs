//! Real `flutter test` proof that `<Model>ComputedParams`'s `operator ==`/
//! `hashCode` are *deep* (wire-based), not identity-based, for nested
//! `params_type` values (`docs/design/computed-fields.md`).
//!
//! `tests/riverpod_generator.rs`'s
//! `riverpod_rest_convenience_providers_expose_typed_computed_params` (and
//! its RPC counterpart) only check that `operator ==`/`hashCode` are
//! *present* in the generated text — they never construct two instances
//! and compare them, so they never caught that the generated `==` compared
//! `field == other.field` on nested params instances (e.g. `ProxyParams`)
//! directly.
//!
//! The real, fail-then-pass regression guard is
//! [`computed_params_deep_equality_holds_under_default_preset`]: under the
//! `default` preset, a `type`-declared params class like `ProxyParams` has
//! no `==` of its own (`models.dart.j2` never annotates its data classes),
//! so the pre-fix identity fallback is directly observable — this test
//! genuinely failed before the `computed_params_class.dart.j2` fix (wire
//! equality via `jsonEncode(toWire())`) and passes after it.
//!
//! [`computed_params_deep_equality_holds_under_riverpod_preset`] runs the
//! identical check under the `riverpod` preset per this story's own
//! instruction to extend "the riverpod value-equality test", and is a
//! valid regression guard for the shared template going forward — but it
//! is **not** fail-then-pass evidence on its own: the riverpod preset
//! separately annotates every generated data class (including
//! `type`-declared params classes) with `@MappableClass()`
//! (`riverpod/enums_and_data_classes.dart.j2`, issue #325) purely for
//! `@riverpod` family-argument caching in general, which already gives
//! `ProxyParams` real value equality independent of anything
//! `computed_params_class.dart.j2` does. That means the pre-fix
//! identity-comparison bug was never actually observable under the
//! riverpod preset — confirmed empirically, not assumed: the pre-fix
//! template passed this exact check when run against `DartPreset::Riverpod`
//! before the fix in this file was written. The `default`-preset test above
//! is what proves the fix; this one guards the riverpod preset against a
//! *future* regression (e.g. if `@MappableClass()` were ever removed from
//! a nested params type) without depending on that coincidence.
//!
//! Mirrors `tests/decimal_round_trip.rs`'s harness: generate a real
//! package, drop in a real `flutter_test` file, run `flutter test` for
//! real. Skips (printed, not silently swallowed) when `flutter` isn't on
//! `PATH`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

const SCHEMA_SOURCE: &str = r#"
type ProxyParams {
  width Int?
}

model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed(params: ProxyParams?)
}
"#;

fn check_test_body(import_line: &str) -> String {
    format!(
        r#"
{import_line}
import 'package:flutter_test/flutter_test.dart';

void main() {{
  test('ImageComputedParams equality is wire-equality, not identity (docs/design/computed-fields.md)', () {{
    // Two separately-constructed instances with structurally-equal nested
    // params must compare equal and hash equal — this is what riverpod's
    // family cache needs to dedupe a freshly-built-but-equal argument
    // instead of refetching (model_providers.dart.j2's module doc).
    final first = ImageComputedParams(proxyUrl: ProxyParams(width: 800));
    final second = ImageComputedParams(proxyUrl: ProxyParams(width: 800));
    expect(first, equals(second));
    expect(first.hashCode, equals(second.hashCode));

    // Unequal nested values must still compare unequal.
    final different = ImageComputedParams(proxyUrl: ProxyParams(width: 400));
    expect(first, isNot(equals(different)));

    // An unset field vs. an explicitly different one must differ too.
    final unset = ImageComputedParams();
    expect(first, isNot(equals(unset)));
  }});

  test('ImageComputedParamsBuilder builds a wire-equal instance', () {{
    final built = ImageComputedParamsBuilder()
        .proxyUrl(ProxyParams(width: 800))
        .build();
    expect(built, equals(ImageComputedParams(proxyUrl: ProxyParams(width: 800))));
    expect(ImageComputedParamsBuilder().build(), equals(ImageComputedParams()));
  }});
}}
"#
    )
}

fn generate_check_package(
    preset: DartPreset,
    library_name: &str,
) -> cratestack_client_dart::GeneratedDartPackage {
    let schema =
        cratestack_parser::parse_schema(SCHEMA_SOURCE).expect("inline schema should parse");
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .unwrap_or_else(|error| panic!("template should render under {preset:?}: {error}"))
}

fn write_package(package: &cratestack_client_dart::GeneratedDartPackage, dir: &std::path::Path) {
    fs::create_dir_all(dir).expect("tmp dir should be created");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }
}

fn run_flutter_pub_get(dir: &std::path::Path) {
    let pub_get = Command::new("flutter")
        .args(["pub", "get"])
        .current_dir(dir)
        .output()
        .expect("run flutter pub get");
    assert!(
        pub_get.status.success(),
        "flutter pub get failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&pub_get.stdout),
        String::from_utf8_lossy(&pub_get.stderr)
    );
}

fn run_flutter_test(dir: &std::path::Path, test_relative_path: &str) {
    let run = Command::new("flutter")
        .args(["test", test_relative_path])
        .current_dir(dir)
        .output()
        .expect("run flutter test");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "flutter test against the generated computed-params equality check failed — this is \
         the real deep-equality proof, not a Rust string assertion:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All tests passed!") || stderr.contains("All tests passed!"),
        "expected flutter test's own success marker, got:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// The genuine fail-then-pass regression guard — see this file's module
/// doc for why the `default` preset (not `riverpod`) is where the
/// identity-equality bug is actually observable.
#[test]
fn computed_params_deep_equality_holds_under_default_preset() {
    if !flutter_available() {
        eprintln!(
            "skipping computed_params_deep_equality_holds_under_default_preset: \
             `flutter` not on PATH (expected in this repo's Rust-only CI jobs — see this \
             test file's module doc)"
        );
        return;
    }

    let package =
        generate_check_package(DartPreset::Default, "computed_params_wire_equality_default");
    let dir = project_tmp_path("computed-params-wire-equality-default");
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("existing tmp dir should be removable");
    }
    write_package(&package, &dir);

    let test_path = dir.join("test/computed_params_wire_equality_test.dart");
    fs::create_dir_all(test_path.parent().expect("test/ parent")).expect("create test dir");
    fs::write(
        &test_path,
        check_test_body("import 'package:computed_params_wire_equality_default/computed_params_wire_equality_default.dart';"),
    )
    .expect("write check test");

    run_flutter_pub_get(&dir);
    run_flutter_test(&dir, "test/computed_params_wire_equality_test.dart");

    fs::remove_dir_all(&dir).expect("tmp dir should be removable");
}

/// Regression guard for the same property under the `riverpod` preset —
/// see this file's module doc for why this does not, by itself, prove the
/// fix (the riverpod preset's unrelated `@MappableClass()` annotation on
/// every data class already gave nested params real equality before this
/// fix landed).
#[test]
fn computed_params_deep_equality_holds_under_riverpod_preset() {
    if !flutter_available() {
        eprintln!(
            "skipping computed_params_deep_equality_holds_under_riverpod_preset: \
             `flutter` not on PATH (expected in this repo's Rust-only CI jobs — see this \
             test file's module doc)"
        );
        return;
    }

    let package = generate_check_package(
        DartPreset::Riverpod,
        "computed_params_wire_equality_riverpod",
    );
    let dir = project_tmp_path("computed-params-wire-equality-riverpod");
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("existing tmp dir should be removable");
    }
    write_package(&package, &dir);

    let test_path = dir.join("test/computed_params_wire_equality_test.dart");
    fs::create_dir_all(test_path.parent().expect("test/ parent")).expect("create test dir");
    fs::write(
        &test_path,
        check_test_body(
            "import 'package:computed_params_wire_equality_riverpod/src/models/image.dart';",
        ),
    )
    .expect("write check test");

    run_flutter_pub_get(&dir);

    // The riverpod preset's `build_runner`-generated `.g.dart`/`.mapper.dart`
    // parts must exist before `flutter test` can even parse the library —
    // same requirement `justfile`'s `verify-dart` recipe documents.
    let build_runner = Command::new("dart")
        .args([
            "run",
            "build_runner",
            "build",
            "--delete-conflicting-outputs",
        ])
        .current_dir(&dir)
        .output()
        .expect("run dart run build_runner build");
    assert!(
        build_runner.status.success(),
        "dart run build_runner build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_runner.stdout),
        String::from_utf8_lossy(&build_runner.stderr)
    );

    run_flutter_test(&dir, "test/computed_params_wire_equality_test.dart");

    fs::remove_dir_all(&dir).expect("tmp dir should be removable");
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
