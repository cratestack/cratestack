//! Real `flutter pub get` + `flutter test` proof that the generated Dart
//! client's `Decimal` support (cratestack#498) behaves as documented, not
//! just that the generated *text* looks right (`tests/generator.rs`'s
//! `decimal_scalar_maps_to_a_real_decimal_type` proves that half).
//!
//! Generates a real package from `tests/fixtures/decimal_scalar.cstack`,
//! drops a `test/decimal_round_trip_test.dart` into it that exercises the
//! generated `Invoice`/`CreateInvoiceInput`/`DecimalFilter`'s real
//! `fromWire`/`toWire` — the same wire-decode/encode pipeline
//! `wire_decode.rs`/`wire_encode.rs` generate for every model — then
//! runs it for real with `flutter test`, the same command
//! `justfile`'s `verify-dart` recipe uses for its own riverpod-preset
//! generated test files. `flutter pub get` first: the generated
//! `pubspec.yaml` depends on `sdk: flutter`, so a standalone `dart pub
//! get` can't resolve it.
//!
//! `flutter test`, not `dart run` on a plain script: an earlier version
//! of this test used `dart run` directly and hit a real Dart VM front-end
//! crash (`_FfiUseSiteTransformer`, "type 'InvalidType' is not a subtype
//! of type 'FunctionType' in type cast") compiling a Flutter-SDK-
//! dependent package outside a proper Flutter test/build harness — not a
//! bug in the generated code, but real evidence `dart run` isn't a
//! supported way to execute a script inside a package that declares
//! `sdk: flutter` (every generated Dart package does, even this
//! "default"-preset one — see `pubspec.yaml.j2`). `flutter test` avoids
//! it entirely, and is the same invocation this repo's own CI already
//! trusts for exactly this kind of package.
//!
//! Skips (printed, not silently swallowed) when `flutter` isn't on
//! `PATH` — no Rust CI job in this repo currently provisions Flutter
//! (the Flutter-provisioned job, `dart-verify`, only runs `just
//! verify-dart`, not this crate's own `cargo test`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

const CHECK_TEST: &str = r#"
import 'package:decimal/decimal.dart';
import 'package:decimal_round_trip_check/decimal_round_trip_check.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('decimal round trip (cratestack#498)', () {
    // Requirement 1: plain and scientific notation for the same value
    // decode to equal Decimals.
    final plain = Decimal.parse('0.0000001');
    final scientific = Decimal.parse('1E-7');
    expect(plain, equals(scientific));

    // Requirement 3: precision survives beyond rust_decimal's ~28-29
    // significant-digit capacity — mirrors
    // crates/cratestack-pg/tests/decimal_bigdecimal_backend.rs's
    // decimal_round_trips_beyond_rust_decimal_capacity_under_bigdecimal_backend
    // (a 40-significant-digit value), in the scientific notation a
    // decimal-bigdecimal server would actually emit for it.
    const wideWire = '1.234567890123456789012345678901234567890E+10';
    final invoice = Invoice.fromWire(<String, Object?>{
      'id': 'inv_1',
      'reference': 'INV-1',
      'amountXaf': wideWire,
      'discountXaf': null,
    });
    expect(invoice.amountXaf, isNotNull);
    final decoded = invoice.amountXaf!;
    // `package:decimal` normalizes away the input's insignificant
    // trailing zero (`...67890E+10` -> `...6789` after the shift) — the
    // *value* is identical either way, which is the property requirement
    // 2 actually cares about (string form may normalize).
    const expectedPlain = '12345678901.2345678901234567890123456789';
    expect(decoded.toString(), expectedPlain);
    expect(invoice.discountXaf, isNull);

    // Requirement 2: encode round-trips — what this client encodes must
    // decode back to the identical value (string form may normalize to
    // plain notation, which is exactly what just happened above).
    final reWired = invoice.toWire();
    expect(reWired['amountXaf'], expectedPlain);
    final roundTripped = Invoice.fromWire(reWired);
    expect(roundTripped.amountXaf, decoded);

    // DecimalFilter (requirement 4): comparison operands are real
    // Decimals too, encoding/decoding through the identical pipeline.
    final filter = DecimalFilter.fromWire(<String, Object?>{
      'eq': '1E-7',
      'gte': '0.0000001',
    });
    expect(filter.eq, filter.gte);

    // Requirement 5: null/optional handling — CreateInvoiceInput's
    // amountXaf is schema-required (non-nullable Decimal), discountXaf
    // schema-optional.
    final input = CreateInvoiceInput.fromWire(<String, Object?>{
      'reference': 'INV-2',
      'amountXaf': '42.5',
      'discountXaf': null,
      'customerId': 'cust_1',
    });
    expect(input.discountXaf, isNull);
    expect(input.amountXaf, Decimal.parse('42.5'));
  });

  // cratestack#499 remediation of #498 (F6): a relation-embedded `Decimal`
  // field must decode through the exact same `Model.fromWire` chokepoint
  // as a direct field — unlike TypeScript's original (pre-remediation)
  // `reviveDecimalFields`, which only matched a flat per-model key set and
  // missed exactly this case. Real generated `Invoice`/`Customer` classes,
  // not a hand-rolled stand-in.
  test('relation-embedded Decimal field decodes via fromWire (cratestack#499 F5/F6)', () {
    final invoice = Invoice.fromWire(<String, Object?>{
      'id': 'inv_2',
      'reference': 'INV-3',
      'amountXaf': '10.00',
      'discountXaf': null,
      'customerId': 'cust_1',
      'customer': <String, Object?>{'id': 'cust_1', 'balance': '1E-7'},
    });
    expect(invoice.customer, isNotNull);
    expect(invoice.customer!.balance, Decimal.parse('0.0000001'));
  });

  // cratestack#499 remediation of #498 (F2/F6): a procedure's own return
  // type (here a `type QuoteResult { price Decimal }`) must decode its
  // `Decimal` field too — real generated `QuoteResult.fromWire`, the same
  // class `ProceduresApi.quote()` decodes its HTTP response body through
  // (`builders_model.rs::build_procedure`'s `return_decode_expr`).
  test('procedure return type Decimal field decodes via fromWire (cratestack#499 F2/F6)', () {
    final result = QuoteResult.fromWire(<String, Object?>{'price': '1.234567890123456789012345E+5'});
    expect(result.price, Decimal.parse('123456.7890123456789012345'));
    final reWired = result.toWire();
    expect(reWired['price'], '123456.7890123456789012345');
  });
}
"#;

#[test]
fn decimal_round_trips_through_the_generated_dart_client() {
    if !flutter_available() {
        eprintln!(
            "skipping decimal_round_trips_through_the_generated_dart_client: \
             `flutter` not on PATH (expected in this repo's Rust-only CI jobs — \
             see tests/decimal_round_trip.rs's module doc)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file("tests/fixtures/decimal_scalar.cstack")
        .expect("fixture schema should parse");
    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "decimal_round_trip_check".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("default template should render");

    let dir = project_tmp_path("decimal-round-trip");
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("existing tmp dir should be removable");
    }
    fs::create_dir_all(&dir).expect("tmp dir should be created");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    // Overwrites the generated (assert-based, not `package:test`-based —
    // see `templates/package_test.dart.j2`) `test/<package>_test.dart`
    // with this crate's own real `flutter_test` suite.
    let test_path = dir.join("test/decimal_round_trip_test.dart");
    fs::create_dir_all(test_path.parent().expect("test/ parent")).expect("create test dir");
    fs::write(&test_path, CHECK_TEST).expect("write check test");

    override_in_repo_dart_packages(&dir);
    let pub_get = Command::new("flutter")
        .args(["pub", "get"])
        .current_dir(&dir)
        .output()
        .expect("run flutter pub get");
    assert!(
        pub_get.status.success(),
        "flutter pub get failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&pub_get.stdout),
        String::from_utf8_lossy(&pub_get.stderr)
    );

    // Issue #668 phase 2: `models.dart`'s `part 'models.builder.dart';`
    // (every `@CratestackBuilder()`-annotated data class, including
    // `Invoice`/`CreateInvoiceInput`/`DecimalFilter` below) needs
    // `package:cratestack_builder` to expand it before `flutter test` can
    // even compile this package — mirrors `justfile`'s `verify-dart`
    // recipe's own `dart run build_runner build` step.
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

    assert_touch_flag_setter_stays_suppressed(&dir);

    let run = Command::new("flutter")
        .args(["test", "test/decimal_round_trip_test.dart"])
        .current_dir(&dir)
        .output()
        .expect("run flutter test");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "flutter test against the generated Decimal client failed — this is the real \
         round-trip proof for cratestack#498 requirements 1, 2, 3, 4, 5, not a Rust \
         string assertion:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All tests passed!") || stderr.contains("All tests passed!"),
        "expected flutter test's own success marker, got:\nstdout: {stdout}\nstderr: {stderr}"
    );

    fs::remove_dir_all(&dir).expect("tmp dir should be removable");
}

/// Regression guard for cratestack_builder#735: on `UpdateInvoiceInput`
/// (this fixture's own `discountXaf Decimal?` field), `cratestack_builder`'s
/// `build_runner` pass must expand `models.builder.dart`'s
/// `UpdateInvoiceInputBuilder` with a fluent `discountXaf(Decimal? value)`
/// setter but NO fluent `discountXafIsSet(bool value)` setter of its own.
///
/// `discountXafIsSet` is the Rust-synthesized touch flag
/// (`cratestack-client-dart`'s `patch_touch.rs`) that lets the wire encoder
/// tell "field left untouched" apart from "field explicitly cleared to
/// null" for a nullable patch field — `discountXaf`'s own setter is already
/// responsible for flipping it (see the generated setter body below). If
/// `discountXafIsSet` regains its own public setter, a caller can write
/// `.discountXaf(value).discountXafIsSet(false)` and produce a patch that
/// LIES about being untouched while it still carries `value` on the wire —
/// exactly the "claims a field is untouched while carrying a value" bug
/// cratestack_builder#735 fixed (the touch flag used to be included in the
/// generator's plain field-iteration loop with no suppression list at all).
///
/// This asserts against the REAL generated `models.builder.dart` — the
/// external `cratestack_builder` pub.dev package's own `build_runner`
/// output — not a Rust-side model of what it should contain, since the bug
/// lives entirely in that external generator, not in this crate's own
/// Jinja templates.
fn assert_touch_flag_setter_stays_suppressed(package_dir: &Path) {
    let builder_path = package_dir.join("lib/src/models.builder.dart");
    let builder_source = fs::read_to_string(&builder_path)
        .unwrap_or_else(|error| panic!("could not read generated {builder_path:?}: {error}"));

    let class_marker = "class UpdateInvoiceInputBuilder {";
    let class_start = builder_source.find(class_marker).unwrap_or_else(|| {
        panic!(
            "generated models.builder.dart has no `{class_marker}` — expected \
             `UpdateInvoiceInput`'s builder class (does the decimal_scalar.cstack \
             fixture still declare `Invoice.discountXaf Decimal?`?):\n{builder_source}"
        )
    });
    // The class body ends at the first column-0 `}` after the opening brace
    // — `dart format` (which `build_runner` runs generated output through)
    // indents every member, so a bare `}` at the start of a line only ever
    // closes the class itself, never a nested member.
    let body_start = class_start + class_marker.len();
    let body_end = builder_source[body_start..]
        .find("\n}\n")
        .map(|offset| body_start + offset)
        .unwrap_or_else(|| {
            panic!(
                "could not find the end of UpdateInvoiceInputBuilder's class body \
                 in generated models.builder.dart:\n{builder_source}"
            )
        });
    let class_body = &builder_source[body_start..body_end];

    assert!(
        class_body.contains("UpdateInvoiceInputBuilder discountXaf(Decimal? value) {"),
        "UpdateInvoiceInputBuilder is missing its ordinary `discountXaf` fluent \
         setter — the generator or fixture changed shape underneath this \
         regression guard:\n{class_body}"
    );
    assert!(
        !class_body.contains("discountXafIsSet(bool value)"),
        "UpdateInvoiceInputBuilder has regained a fluent `discountXafIsSet` \
         setter of its own — this is cratestack_builder#735 (\"a {{field}}IsSet \
         touch flag gets no setter of its own\") regressing. A caller could now \
         write `.discountXaf(value).discountXafIsSet(false)` and produce a patch \
         that CLAIMS `discountXaf` is untouched while it still carries a real \
         value on the wire. The touch flag must stay derived state, set only as \
         a side effect of `discountXaf`'s own setter:\n{class_body}"
    );
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

/// Points `cratestack_builder` and `cratestack_annotations` at this repo's
/// own `dart-packages/`, so `pub get` resolves the constraints in the
/// WORKING TREE rather than the ones the last release published.
///
/// Without it this test checks history, not the change in front of it, and
/// it fails in exactly one situation: a commit that raises the generator's
/// annotations floor and `cratestack_builder`'s own `cratestack_annotations`
/// constraint together. pub.dev still serves the previous builder, which
/// forbids the annotations release the generator now asks for —
///
/// ```text
/// Because cratestack_builder >=0.8.14 depends on cratestack_annotations
/// ^0.8.10 and <client> depends on cratestack_annotations ^0.9.1,
/// cratestack_builder >=0.8.14 is forbidden.
/// ```
///
/// — the chicken-and-egg `package_floors.rs`'s module doc names. A
/// `pubspec_overrides.yaml` rather than an edit to the generated
/// `pubspec.yaml`, so the generated file stays byte-for-byte what the
/// generator emitted. `cratestack_cbor` is deliberately NOT overridden: it
/// resolves from pub.dev, which is what proves the emitted floor names a
/// really published release.
fn override_in_repo_dart_packages(dir: &std::path::Path) {
    let dart_packages = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dart-packages")
        .canonicalize()
        .expect("dart-packages/ should exist in this repo");
    fs::write(
        dir.join("pubspec_overrides.yaml"),
        format!(
            "dependency_overrides:\n  \
             cratestack_annotations:\n    path: {0}/cratestack_annotations\n  \
             cratestack_builder:\n    path: {0}/cratestack_builder\n",
            dart_packages.display()
        ),
    )
    .expect("write pubspec_overrides.yaml");
}
