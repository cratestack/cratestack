//! Every generated package encodes and decodes CBOR through the published
//! `cratestack_cbor` package (flutter_rust_bridge natively, wasm-bindgen on
//! web, issue #563). This was once a choice (`native_cbor`, and the CLI's
//! `--no-native-cbor` opt-out to pure-Dart `package:cbor`); the pure-Dart
//! mode is gone, so this file pins the one remaining output: the
//! `cratestack_cbor` dependency in `pubspec.yaml`, its import and cached
//! async codec in `lib/src/runtime.dart`, and no trace of `package:cbor`.
//!
//! Structural coverage only (source-level assertions). The real-compiler
//! proof (`flutter pub get` + `flutter analyze` + a functional HTTP round
//! trip through the real `cratestack_cbor` codec, both REST and RPC, both
//! happy path and the async exception-decode path) runs under
//! `just verify-dart`, from the `native_cbor_echo{,_rpc}.cstack` and
//! `native_cbor_echo_{rest,rpc}_test.dart` fixtures, and the codec cache's
//! retry behaviour under `tests/native_cbor_codec_memoization.rs`.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const REST_FIXTURE: &str = "tiny_rest";
const RPC_FIXTURE: &str = "tiny_rpc";

/// cratestack#779: the `cratestack_cbor` API floor a generated pubspec
/// declares, restated here as a **literal** rather than recomputed from
/// `env!("CARGO_PKG_VERSION")` the way this file did before.
///
/// That is the entire point, not an oversight, and it is a TRIPWIRE as much
/// as a regression guard. The old assertion derived its expected value from
/// the same input the generator derived *its* value from, so it agreed with
/// the generator by construction and could not observe the defect #779 is
/// about. A literal disagrees the moment the generator starts moving with
/// `just bump` again — and, just as importantly, raising the real floor in
/// `src/package_floors.rs` turns this test red, forcing the second edit to
/// be a deliberate act with a reason attached. A derived value follows
/// silently.
///
/// **Do not "fix" a failure here by deriving this from
/// `package_floors.rs`.** That was tried (#845) and reverted: it was done
/// after this literal went stale during the 0.9.3 floor work, but going
/// stale is exactly the tripwire firing — it had caught an incomplete
/// revert of the real constant. Removing it traded a working guard for a
/// quieter test run. The real defect was that the failure *looked* like
/// three unrelated "pubspec must depend on cratestack_cbor" assertions
/// dumping whole files, which is what made it read as noise.
///
/// [`literal_matches_the_real_floor`] fixes that instead: it compares this
/// literal against the constant and says so in one line, so a disagreement
/// is legible without being automatic. Update this literal when that test
/// tells you to — after checking the new floor names a version pub.dev
/// actually serves. The matching TypeScript literal
/// (`cratestack-client-typescript/tests/native_cbor_generator.rs`) carries
/// the same instruction and the same guard.
const CRATESTACK_CBOR_FLOOR: &str = "0.9.3";

/// Reads `CRATESTACK_CBOR_FLOOR` out of `src/package_floors.rs`.
///
/// Used ONLY by [`literal_matches_the_real_floor`], never to build an
/// expectation — deriving the expectation is the thing this file exists not
/// to do (see the constant above). A line scan rather than a Rust parse,
/// matching `package_floors_tests.rs`'s own `pubspec_value`: one constant,
/// known shape, and pulling in a parser to read it would be the larger risk.
fn real_cbor_floor() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/package_floors.rs");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("reading {path} for CRATESTACK_CBOR_FLOOR: {error}"));
    source
        .lines()
        .find_map(|line| {
            let rest = line
                .trim()
                .strip_prefix("pub(crate) const CRATESTACK_CBOR_FLOOR: &str = \"")?;
            rest.strip_suffix("\";").map(str::to_owned)
        })
        .unwrap_or_else(|| {
            panic!(
                "could not find `pub(crate) const CRATESTACK_CBOR_FLOOR: &str = \"...\";` in \
                 {path} — was it renamed or reformatted? This drift check is meaningless until \
                 it can read the real floor."
            )
        })
}

/// The tripwire, stated as its own assertion so a disagreement reads as one
/// clear line instead of three "pubspec must depend on cratestack_cbor"
/// failures dumping whole generated files — which is how this drift was
/// actually experienced during the 0.9.3 work, and why it got misread as
/// noise and "fixed" by deriving (#845, reverted).
/// The full requirement the generator emits for this floor: the
/// hand-written literal above, plus the ceiling derived from the release
/// line.
///
/// Composing the ceiling here rather than hardcoding it is deliberate and
/// does NOT weaken the tripwire. The two halves have opposite rules — the
/// floor is a hand-verified fact about published archives, the ceiling is
/// mechanical and by design names a version that does not exist yet.
/// Hardcoding the ceiling would turn this test red at every minor bump for
/// no defect, which is exactly the noise that got the tripwire misread and
/// deleted once already (#845, reverted by #849). The arithmetic it relies
/// on is pinned separately against a hand-written table in
/// `src/release_line_tests.rs`, so it is not agreeing with itself.
fn expected_requirement() -> String {
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    let mut component = |which: &str| -> u64 {
        parts
            .next()
            .unwrap_or_else(|| panic!("CARGO_PKG_VERSION has no {which} component"))
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or_else(|error| panic!("CARGO_PKG_VERSION's {which} component: {error}"))
    };
    let major = component("major");
    let minor = component("minor");
    format!(">={CRATESTACK_CBOR_FLOOR} <{major}.{}.0", minor + 1)
}

#[test]
fn literal_matches_the_real_floor() {
    let real = real_cbor_floor();
    assert_eq!(
        CRATESTACK_CBOR_FLOOR, real,
        "this file's CRATESTACK_CBOR_FLOOR literal ({CRATESTACK_CBOR_FLOOR}) disagrees with \
         src/package_floors.rs ({real}).\n\nThis is the tripwire, not a bug: raising the real \
         floor is meant to force a deliberate second edit here. Confirm {real} names a version \
         pub.dev actually serves, then update the literal in this file to match. Do NOT derive \
         it — see the constant's doc comment for why."
    );
}

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

const PRESETS: [DartPreset; 3] = [
    DartPreset::Default,
    DartPreset::Riverpod,
    DartPreset::Fespalier,
];

/// Every preset, both transports: the pubspec depends on `cratestack_cbor`
/// at the floor, and on nothing from `package:cbor`. The riverpod and
/// fespalier presets render their own `pubspec.yaml` from a separate
/// template, so each is checked, not inferred from the default's.
#[test]
fn every_pubspec_depends_on_cratestack_cbor_and_not_on_package_cbor() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        for preset in PRESETS {
            let package = generate(fixture, preset);
            let pubspec = file(&package, "pubspec.yaml");
            assert!(
                pubspec.contains(&format!("cratestack_cbor: '{}'", expected_requirement())),
                "{fixture}/{preset:?}: pubspec.yaml must depend on cratestack_cbor at the \
                 floor:\n{pubspec}"
            );
            assert!(
                !pubspec.contains("cbor: ^"),
                "{fixture}/{preset:?}: pubspec.yaml must not depend on package:cbor:\n{pubspec}"
            );
        }
    }
}

/// Every preset, both transports: one runtime, importing `cratestack_cbor`,
/// with the cached async codec accessor, and no `package:cbor` call or the
/// `cratestackNormalizeWire` shim that existed only to reshape its maps. The
/// riverpod presets reuse the default preset's `lib/src/runtime.dart`
/// verbatim (see `crate::riverpod`'s module doc), which this also holds to.
#[test]
fn every_runtime_imports_cratestack_cbor_and_nothing_from_package_cbor() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        for preset in PRESETS {
            let package = generate(fixture, preset);
            let runtime = file(&package, "lib/src/runtime.dart");
            let label = format!("{fixture}/{preset:?}");
            for needle in [
                "import 'package:cratestack_cbor/cratestack_cbor.dart' as cratestack_cbor;",
                "Future<cratestack_cbor.CratestackCborCodec> _cratestackCborCodec()",
                "Future<Uint8List?> _encodeBody(CratestackWireFormat format, Object? body) async {",
            ] {
                assert!(
                    runtime.contains(needle),
                    "{label}: runtime.dart lacks `{needle}`:\n{runtime}"
                );
            }
            for needle in ["package:cbor", "cbor.cbor.", "cratestackNormalizeWire"] {
                assert!(
                    !runtime.contains(needle),
                    "{label}: runtime.dart still names `{needle}`:\n{runtime}"
                );
            }
        }
    }
}

/// cratestack#1204: the codec work lives in one fragment (`http_runtime/
/// codec.dart.j2`) shared by the REST and RPC adapters, and is async — so the
/// old "sync under `package:cbor`, async under `cratestack_cbor`" fork (and
/// its six `throw await _exceptionFromDio` call sites) is gone.
#[test]
fn the_shared_codec_fragment_goes_through_one_cached_cratestack_cbor_codec() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(fixture, DartPreset::Default);
        let runtime = file(&package, "lib/src/runtime.dart");
        assert!(
            runtime.contains(
                "Future<Object?> _decodeBody(Uint8List bytes, String? contentType, String owner) async {"
            ),
            "{fixture}: decoding must be async:\n{runtime}"
        );
        assert!(
            !runtime.contains("_exceptionFromDio"),
            "{fixture}: the Dio-specific error mapper must be gone:\n{runtime}"
        );
        assert_eq!(
            runtime.matches("class _Cratestack").count(),
            0,
            "{fixture}: no private adapter copies"
        );
        assert!(runtime.contains("jsonDecode(codec.decodeJson(bytes))"));
        assert!(runtime.contains("codec.encodeJson(jsonEncode(body))"));
        assert!(runtime.contains("final codec = await _cratestackCborCodec();"));
        assert_eq!(
            runtime
                .matches("Future<cratestack_cbor.CratestackCborCodec>? _cratestackCborCodecFuture;")
                .count(),
            1,
            "{fixture}: the cached codec future must be declared exactly once, shared by every \
             adapter in the file (cratestack#798 asserts both transports, so a second copy is the \
             drift it guards against)"
        );
    }
}

/// `native_cbor` is still in the template context, as a constant `true`: a
/// `--template-dir` template written when it was a choice, still testing
/// `{% if native_cbor %}`, must keep rendering its `cratestack_cbor` branch.
/// Templates render with minijinja's default lenient undefined, so a removed
/// variable would not error, it would silently take the `package:cbor`
/// branch. Checked for both pubspec templates (the riverpod preset's has its
/// own context) and a runtime one.
#[test]
fn a_custom_template_that_still_tests_native_cbor_takes_the_cratestack_cbor_branch() {
    const PROBE: &str = "{% if native_cbor %}cratestack_cbor{% else %}package:cbor{% endif %}";
    let cases = [
        (DartPreset::Default, "pubspec.yaml.j2", "pubspec.yaml"),
        (
            DartPreset::Default,
            "rest-library.dart.j2",
            "lib/tiny_rest_client.dart",
        ),
        (
            DartPreset::Riverpod,
            "riverpod/pubspec.yaml.j2",
            "pubspec.yaml",
        ),
    ];
    for (preset, template, output) in cases {
        let template_dir = project_tmp_path("native-cbor-probe");
        let template_path = template_dir.join(template);
        fs::create_dir_all(template_path.parent().expect("template parent"))
            .expect("template dir should be created");
        fs::write(&template_path, PROBE).expect("probe template should write");

        let package = generate_with(REST_FIXTURE, preset, Some(template_dir.clone()));
        assert_eq!(
            file(&package, output),
            "cratestack_cbor",
            "{preset:?}: a custom {template} testing `native_cbor` took the wrong branch"
        );

        fs::remove_dir_all(&template_dir).expect("template dir should be removable");
    }
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

fn generate(fixture_stem: &str, preset: DartPreset) -> GeneratedDartPackage {
    generate_with(fixture_stem, preset, None)
}

fn generate_with(
    fixture_stem: &str,
    preset: DartPreset,
    template_dir: Option<PathBuf>,
) -> GeneratedDartPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: format!("{fixture_stem}_client"),
            base_path: "/api".to_owned(),
            template_dir,
            preset,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            ..DartGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{fixture_stem}: generation should succeed: {error}"))
}

fn file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
        .contents
        .as_str()
}
